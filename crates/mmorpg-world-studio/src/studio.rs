//! The workflow behind the app: jobs that capture, import and start the agent, and the exports
//! and edits they produce. Only one job runs at a time: they share the browser, GPU and target dir.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

use crate::brief::{Brief, ImportReport, Rectangle};
use crate::capture;
use crate::import;

const TILES_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const AGENT_TIMEOUT: Duration = Duration::from_secs(90 * 60);
const MAX_LOG_LINES: usize = 4_000;
/// The pyramid and the zone's terrain both end inside this distance of the origin (metres).
const WORLD_HALF_METRES: i32 = 1_024;
const SELECTION_METRES: std::ops::RangeInclusive<i32> = 16..=512;
const AGENT_BUDGET_USD: &str = "5";
const AGENT_TOOLS: &str = "Read,Edit,Write,Glob,Grep,Bash(cargo test:*),Bash(cargo check:*),\
Bash(cargo fmt:*),Bash(git status:*),Bash(git diff:*),Bash(git log:*),Bash(git add:*),Bash(git commit:*)";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    id: u64,
    label: String,
    status: JobStatus,
    log: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub min_x: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_z: i32,
    pub pixels_per_metre: u32,
}

impl Selection {
    /// Why this rectangle cannot be exported, if it cannot.
    pub fn problem(&self) -> Option<&'static str> {
        let (width, depth) = (self.max_x - self.min_x, self.max_z - self.min_z);
        if !SELECTION_METRES.contains(&width) || !SELECTION_METRES.contains(&depth) {
            return Some("a selection must be 16 to 512 metres each way");
        }
        if [self.min_x, self.min_z, self.max_x, self.max_z]
            .iter()
            .any(|value| value.abs() > WORLD_HALF_METRES)
        {
            return Some("the selection lies outside the world");
        }
        if ![4, 8, 16].contains(&self.pixels_per_metre) {
            return Some("pixels per metre must be 4, 8 or 16");
        }
        None
    }

    fn rectangle(self) -> Rectangle {
        Rectangle {
            min_x: f64::from(self.min_x),
            min_z: f64::from(self.min_z),
            max_x: f64::from(self.max_x),
            max_z: f64::from(self.max_z),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportState {
    Rendering,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    id: String,
    name: String,
    selection: Selection,
    state: ExportState,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EditState {
    Previewing,
    Previewed,
    Applying,
    Applied,
    Reconciling,
    Reconciled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    id: String,
    export: String,
    state: EditState,
    report: Option<ImportReport>,
    branch: Option<String>,
    worktree: Option<String>,
    /// The prompt the agent received.
    prompt: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
struct Saved {
    exports: BTreeMap<String, Export>,
    edits: BTreeMap<String, Edit>,
}

#[derive(Default)]
struct Inner {
    saved: Saved,
    jobs: Vec<Job>,
    next_job: u64,
    next_id: u64,
}

/// One external command of a job.
struct Step {
    program: PathBuf,
    arguments: Vec<String>,
    directory: PathBuf,
    timeout: Duration,
}

/// What a request can get wrong; the message is shown to the user.
#[derive(Debug)]
pub enum Refusal {
    NotFound,
    Invalid(String),
}

pub struct Studio {
    repo: PathBuf,
    agent: PathBuf,
    inner: Mutex<Inner>,
    /// Held by the running job.
    turn: tokio::sync::Mutex<()>,
}

impl Studio {
    /// Opens the studio for a checkout, restoring earlier exports and edits.
    pub fn open(repo: PathBuf, agent: PathBuf) -> Arc<Self> {
        let mut saved: Saved = std::fs::read(repo.join("artifacts/studio/state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // The job that would have finished these died with the previous process.
        for export in saved.exports.values_mut() {
            if export.state == ExportState::Rendering {
                export.state = ExportState::Failed;
            }
        }
        for edit in saved.edits.values_mut() {
            if matches!(edit.state, EditState::Previewing | EditState::Applying) {
                edit.state = EditState::Failed;
            }
            if edit.state == EditState::Reconciling {
                edit.state = EditState::Applied;
            }
        }
        Arc::new(Self {
            repo,
            agent,
            inner: Mutex::new(Inner {
                saved,
                ..Inner::default()
            }),
            turn: tokio::sync::Mutex::new(()),
        })
    }

    pub fn repo(&self) -> &Path {
        &self.repo
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn tiles_dir(&self) -> PathBuf {
        self.repo.join("artifacts/studio/world")
    }

    pub fn tiles_root(&self) -> PathBuf {
        self.tiles_dir().join("tiles")
    }

    fn export_dir(&self, id: &str) -> PathBuf {
        self.repo.join("artifacts/studio/exports").join(id)
    }

    fn worktree_dir(&self, id: &str) -> PathBuf {
        self.repo.join("artifacts/studio/worktrees").join(id)
    }

    fn fresh_id(&self, prefix: char) -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        let mut inner = self.lock();
        inner.next_id += 1;
        format!("{prefix}{millis}{}", inner.next_id)
    }

    /// The whole state, as the frontend polls it.
    pub fn snapshot(&self) -> serde_json::Value {
        let tiles = self.tiles_root().join("0/0/0.png").is_file();
        let inner = self.lock();
        serde_json::json!({
            "tilesReady": tiles,
            "jobs": inner.jobs,
            "exports": inner.saved.exports.values().rev().collect::<Vec<_>>(),
            "edits": inner.saved.edits.values().rev().collect::<Vec<_>>(),
        })
    }

    /// Writes exports and edits so a restart keeps them; a failure only costs that memory.
    fn persist(inner: &Inner, repo: &Path) {
        let directory = repo.join("artifacts/studio");
        let temporary = directory.join("state.json.tmp");
        let written = std::fs::create_dir_all(&directory)
            .and_then(|()| {
                std::fs::write(
                    &temporary,
                    serde_json::to_vec_pretty(&inner.saved).unwrap_or_default(),
                )
            })
            .and_then(|()| std::fs::rename(&temporary, directory.join("state.json")));
        if let Err(error) = written {
            eprintln!("world-studio: could not save state: {error}");
        }
    }

    fn update(&self, change: impl FnOnce(&mut Saved)) {
        let mut inner = self.lock();
        change(&mut inner.saved);
        Self::persist(&inner, &self.repo);
    }

    fn set_export(&self, id: &str, state: ExportState) {
        self.update(|saved| {
            if let Some(export) = saved.exports.get_mut(id) {
                export.state = state;
            }
        });
    }

    fn change_edit(&self, id: &str, change: impl FnOnce(&mut Edit)) {
        self.update(|saved| {
            if let Some(edit) = saved.edits.get_mut(id) {
                change(edit);
            }
        });
    }

    fn set_job(&self, id: u64, status: JobStatus) {
        if let Some(job) = self.lock().jobs.iter_mut().find(|job| job.id == id) {
            job.status = status;
        }
    }

    fn log(&self, id: u64, line: impl Into<String>) {
        if let Some(job) = self.lock().jobs.iter_mut().find(|job| job.id == id)
            && job.log.len() < MAX_LOG_LINES
        {
            job.log.push(line.into());
        }
    }

    /// Queues `work` behind any running job; its error fails the job.
    fn spawn<F>(
        self: &Arc<Self>,
        label: String,
        work: impl FnOnce(Arc<Self>, u64) -> F + Send + 'static,
    ) where
        F: Future<Output = Result<(), String>> + Send + 'static,
    {
        let id = {
            let mut inner = self.lock();
            inner.next_job += 1;
            let id = inner.next_job;
            inner.jobs.push(Job {
                id,
                label,
                status: JobStatus::Queued,
                log: Vec::new(),
            });
            id
        };
        let studio = Arc::clone(self);
        tokio::spawn(async move {
            let _turn = studio.turn.lock().await;
            studio.set_job(id, JobStatus::Running);
            let result = work(Arc::clone(&studio), id).await;
            if let Err(error) = &result {
                studio.log(id, format!("failed: {error}"));
            }
            studio.set_job(
                id,
                if result.is_ok() {
                    JobStatus::Finished
                } else {
                    JobStatus::Failed
                },
            );
        });
    }

    /// Runs one command, logging stderr (and stdout through `on_line`), and returns its stdout.
    async fn run(
        self: &Arc<Self>,
        job: u64,
        step: Step,
        on_line: impl Fn(&str) -> Option<String>,
    ) -> Result<(), String> {
        let shown = step.program.display().to_string();
        self.log(
            job,
            format!(
                "$ {shown} {}",
                step.arguments.first().map_or("", String::as_str)
            ),
        );
        let mut child = Command::new(&step.program)
            .args(&step.arguments)
            .current_dir(&step.directory)
            // Worktrees share the checkout's build cache.
            .env("CARGO_TARGET_DIR", self.repo.join("target"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("could not start {shown}: {error}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let collect = async {
            tokio::join!(
                self.read_lines(job, stdout, &on_line),
                self.read_stderr(job, stderr)
            );
            let status = child.wait().await.map_err(|error| error.to_string())?;
            Ok::<_, String>(status)
        };
        let status = tokio::time::timeout(step.timeout, collect)
            .await
            .map_err(|_| format!("{shown} timed out"))??;
        if !status.success() {
            return Err(format!("{shown} exited with {status}"));
        }
        Ok(())
    }

    async fn read_lines(
        &self,
        job: u64,
        stream: impl AsyncRead + Unpin,
        on_line: &impl Fn(&str) -> Option<String>,
    ) {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(shown) = on_line(&line) {
                self.log(job, shown);
            }
        }
    }

    async fn read_stderr(&self, job: u64, stream: impl AsyncRead + Unpin) {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            self.log(job, line);
        }
    }

    /// Rebuilds the game and renders the whole zone into the map's tile pyramid.
    pub fn refresh_tiles(self: &Arc<Self>) {
        self.spawn("Render map tiles".to_owned(), |studio, job| async move {
            let build = Step {
                program: "bun".into(),
                arguments: vec!["run".into(), "build".into()],
                directory: studio.repo.join("web"),
                timeout: TILES_TIMEOUT,
            };
            studio.run(job, build, |line| Some(line.to_owned())).await?;
            let request = capture::Request {
                tiles_only: true,
                out: studio.tiles_dir(),
                tiles_url: "/tiles/{z}/{x}/{y}.png".to_owned(),
                ..studio.capture_defaults()
            };
            studio.capture(job, request, TILES_TIMEOUT).await
        });
    }

    /// Renders a selection into a Krita document.
    pub fn export(self: &Arc<Self>, selection: Selection) -> Result<String, Refusal> {
        if let Some(problem) = selection.problem() {
            return Err(Refusal::Invalid(problem.to_owned()));
        }
        let id = self.fresh_id('x');
        let name = format!(
            "area {}x{}",
            selection.max_x - selection.min_x,
            selection.max_z - selection.min_z
        );
        self.update(|saved| {
            saved.exports.insert(
                id.clone(),
                Export {
                    id: id.clone(),
                    name,
                    selection,
                    state: ExportState::Rendering,
                },
            );
        });
        let export = id.clone();
        self.spawn(format!("Export {id}"), move |studio, job| async move {
            let request = capture::Request {
                out: studio.export_dir(&export),
                name: "selection".to_owned(),
                pixels_per_metre: selection.pixels_per_metre,
                bounds: Some([
                    selection.min_x,
                    selection.min_z,
                    selection.max_x,
                    selection.max_z,
                ]),
                ..studio.capture_defaults()
            };
            let result = studio.capture(job, request, EXPORT_TIMEOUT).await;
            studio.set_export(
                &export,
                if result.is_ok() {
                    ExportState::Ready
                } else {
                    ExportState::Failed
                },
            );
            result
        });
        Ok(id)
    }

    fn capture_defaults(&self) -> capture::Request {
        capture::Request {
            root: self.repo.clone(),
            out: self.repo.join("artifacts/studio"),
            only: Vec::new(),
            pixels_per_metre: 8,
            margin: 8.,
            tiles: false,
            tiles_only: false,
            tiles_url: String::new(),
            bounds: None,
            name: "selection".to_owned(),
            list: false,
            target_dir: Some(self.repo.join("target")),
        }
    }

    async fn capture(
        self: &Arc<Self>,
        job: u64,
        request: capture::Request,
        timeout: Duration,
    ) -> Result<(), String> {
        let studio = Arc::clone(self);
        let log = move |line: String| studio.log(job, line);
        tokio::time::timeout(timeout, capture::run(request, &log))
            .await
            .map_err(|_| "the capture timed out".to_owned())?
    }

    /// A rendered export's folder, if it finished.
    pub fn export_folder(&self, id: &str) -> Option<PathBuf> {
        let ready = self
            .lock()
            .saved
            .exports
            .get(id)
            .is_some_and(|export| export.state == ExportState::Ready);
        ready.then(|| self.export_dir(id))
    }

    /// Takes the Krita-saved document for an export and previews what importing it changes.
    pub fn upload(self: &Arc<Self>, export: &str, document: &[u8]) -> Result<String, Refusal> {
        let folder = self.export_folder(export).ok_or(Refusal::NotFound)?;
        if !document.starts_with(b"PK") {
            return Err(Refusal::Invalid(
                "that is not an OpenRaster (.ora) file".to_owned(),
            ));
        }
        let id = self.fresh_id('e');
        let painted = folder.join(format!("{id}.ora"));
        std::fs::write(&painted, document)
            .map_err(|error| Refusal::Invalid(format!("could not store the document: {error}")))?;
        self.update(|saved| {
            saved.edits.insert(
                id.clone(),
                Edit {
                    id: id.clone(),
                    export: export.to_owned(),
                    state: EditState::Previewing,
                    report: None,
                    branch: None,
                    worktree: None,
                    prompt: None,
                },
            );
        });
        let edit = id.clone();
        self.spawn(format!("Preview {id}"), move |studio, job| async move {
            let result = studio
                .import(job, &studio.repo.clone(), &painted, true)
                .await;
            match result {
                Ok(report) => {
                    studio.change_edit(&edit, |edit| {
                        edit.report = Some(report);
                        edit.state = EditState::Previewed;
                    });
                    Ok(())
                }
                Err(error) => {
                    studio.change_edit(&edit, |edit| edit.state = EditState::Failed);
                    Err(error)
                }
            }
        });
        Ok(id)
    }

    async fn import(
        self: &Arc<Self>,
        job: u64,
        root: &Path,
        document: &Path,
        check: bool,
    ) -> Result<ImportReport, String> {
        self.log(job, format!("Reading {}", document.display()));
        let target = self.repo.join("target");
        tokio::time::timeout(
            IMPORT_TIMEOUT,
            import::run(root, document, check, Some(&target)),
        )
        .await
        .map_err(|_| "the import timed out".to_owned())?
    }

    /// Applies a previewed edit to the terrain on its own branch and worktree.
    pub fn apply(self: &Arc<Self>, id: &str) -> Result<(), Refusal> {
        self.claim(id, EditState::Previewed, EditState::Applying)?;
        let edit = id.to_owned();
        self.spawn(format!("Apply {id}"), move |studio, job| async move {
            let result = studio.apply_steps(job, &edit).await;
            if result.is_err() {
                studio.change_edit(&edit, |edit| edit.state = EditState::Failed);
            }
            result
        });
        Ok(())
    }

    async fn apply_steps(self: &Arc<Self>, job: u64, id: &str) -> Result<(), String> {
        let export = self
            .lock()
            .saved
            .edits
            .get(id)
            .map(|edit| edit.export.clone())
            .ok_or("the edit disappeared")?;
        let worktree = self.worktree_dir(id);
        let branch = format!("world-edit/{id}");
        let git = |arguments: Vec<String>, directory: PathBuf| Step {
            program: "git".into(),
            arguments,
            directory,
            timeout: GIT_TIMEOUT,
        };
        let add = git(
            vec![
                "worktree".into(),
                "add".into(),
                "-b".into(),
                branch.clone(),
                worktree.display().to_string(),
                "HEAD".into(),
            ],
            self.repo.clone(),
        );
        self.run(job, add, |line| Some(line.to_owned())).await?;
        let document = self.export_dir(&export).join(format!("{id}.ora"));
        let report = self.import(job, &worktree, &document, false).await?;
        let stat = git(vec!["diff".into(), "--stat".into()], worktree.clone());
        self.run(job, stat, |line| Some(line.to_owned())).await?;
        self.change_edit(id, |edit| {
            edit.report = Some(report);
            edit.branch = Some(branch);
            edit.worktree = Some(worktree.display().to_string());
            edit.state = EditState::Applied;
        });
        Ok(())
    }

    /// Moves an edit from `from` to `to`, refusing a stale or repeated request.
    fn claim(&self, id: &str, from: EditState, to: EditState) -> Result<(), Refusal> {
        let mut inner = self.lock();
        let edit = inner.saved.edits.get_mut(id).ok_or(Refusal::NotFound)?;
        if edit.state != from {
            return Err(Refusal::Invalid(format!(
                "this edit is {:?}, not {from:?}",
                edit.state
            )));
        }
        edit.state = to;
        Self::persist(&inner, &self.repo);
        Ok(())
    }

    /// Starts the agent on an applied edit, with the owner's creative direction.
    pub fn reconcile(self: &Arc<Self>, id: &str, direction: &str) -> Result<(), Refusal> {
        self.claim(id, EditState::Applied, EditState::Reconciling)?;
        let edit = id.to_owned();
        let direction = direction.to_owned();
        self.spawn(format!("Reconcile {id}"), move |studio, job| async move {
            let result = studio.run_agent(job, &edit, &direction).await;
            studio.change_edit(&edit, |edit| {
                // A failed run leaves the applied terrain, so the agent can be started again.
                edit.state = if result.is_ok() {
                    EditState::Reconciled
                } else {
                    EditState::Applied
                };
            });
            result
        });
        Ok(())
    }

    async fn run_agent(
        self: &Arc<Self>,
        job: u64,
        id: &str,
        direction: &str,
    ) -> Result<(), String> {
        let (edit, selection) = {
            let inner = self.lock();
            let edit = inner
                .saved
                .edits
                .get(id)
                .ok_or("the edit disappeared")?
                .clone();
            let export = inner
                .saved
                .exports
                .get(&edit.export)
                .ok_or("the export disappeared")?;
            (edit, export.selection)
        };
        let report = edit.report.as_ref().ok_or("the edit has no report")?;
        let branch = edit.branch.as_deref().ok_or("the edit has no branch")?;
        let worktree = PathBuf::from(edit.worktree.as_deref().ok_or("the edit has no worktree")?);
        let images = self.export_dir(&edit.export).join("selection");
        let prompt = Brief {
            report,
            selection: selection.rectangle(),
            branch,
            before_images: &images.display().to_string(),
            direction,
        }
        .prompt();
        self.change_edit(id, |edit| edit.prompt = Some(prompt.clone()));
        let step = Step {
            program: self.agent.clone(),
            arguments: vec![
                "-p".into(),
                prompt,
                "--output-format".into(),
                "stream-json".into(),
                "--verbose".into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
                "--allowed-tools".into(),
                AGENT_TOOLS.into(),
                "--add-dir".into(),
                images.display().to_string(),
                "--max-budget-usd".into(),
                AGENT_BUDGET_USD.into(),
            ],
            directory: worktree,
            timeout: AGENT_TIMEOUT,
        };
        self.run(job, step, agent_line).await
    }
}

/// The readable part of one line of the agent's stream, if it has one.
fn agent_line(line: &str) -> Option<String> {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
        return Some(line.to_owned());
    };
    match event["type"].as_str()? {
        "assistant" => {
            let parts: Vec<String> = event["message"]["content"]
                .as_array()?
                .iter()
                .filter_map(|block| match block["type"].as_str()? {
                    "text" => block["text"].as_str().map(str::to_owned),
                    "tool_use" => block["name"].as_str().map(|name| format!("[{name}]")),
                    _ => None,
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        "result" => event["result"].as_str().map(str::to_owned),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Selection, agent_line};

    const VALID: Selection = Selection {
        min_x: -40,
        min_z: 0,
        max_x: 40,
        max_z: 80,
        pixels_per_metre: 8,
    };

    #[test]
    fn selections_must_be_a_sensible_part_of_the_world() {
        assert_eq!(VALID.problem(), None);
        for bad in [
            Selection {
                max_x: -39,
                ..VALID
            },
            Selection {
                max_z: 1_000,
                ..VALID
            },
            Selection {
                max_x: 2_000,
                min_x: 1_800,
                ..VALID
            },
            Selection {
                pixels_per_metre: 3,
                ..VALID
            },
        ] {
            assert!(bad.problem().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn the_agents_stream_is_reduced_to_what_a_person_reads() {
        let assistant = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Moving a road"},{"type":"tool_use","name":"Edit"}]}}"#;
        assert_eq!(agent_line(assistant).unwrap(), "Moving a road [Edit]");
        assert_eq!(
            agent_line(r#"{"type":"result","result":"Done"}"#).unwrap(),
            "Done"
        );
        assert_eq!(agent_line(r#"{"type":"system"}"#), None);
        assert_eq!(agent_line("plain output").unwrap(), "plain output");
    }
}
