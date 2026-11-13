//! Public native landing contract, shared by host emission and both native join routes.
use game_server::MatchId;
use url::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinInvite {
    pub lobby_id: String,
    pub match_id: MatchId,
}

impl JoinInvite {
    pub fn parse(value: &str) -> Result<Self, String> {
        let invalid = || {
            "Invalid invitation. Paste the full moenarch.com invitation supplied by the host."
                .to_owned()
        };
        if !value.starts_with("https://moenarch.com/native.html?")
            || value.trim() != value
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(invalid());
        }
        let url = Url::parse(value).map_err(|_| invalid())?;
        if url.origin().ascii_serialization() != "https://moenarch.com"
            || url.path() != "/native.html"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let mut lobby = None;
        let mut matched = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "lobbyId" if lobby.is_none() => lobby = Some(value.into_owned()),
                "matchId" if matched.is_none() => matched = Some(value.into_owned()),
                _ => return Err(invalid()),
            }
        }
        let lobby_id = lobby.ok_or_else(invalid)?;
        if lobby_id.len() != 12
            || !lobby_id
                .bytes()
                .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
        {
            return Err(invalid());
        }
        let match_id = MatchId::new(matched.ok_or_else(invalid)?).map_err(|_| invalid())?;
        Ok(Self { lobby_id, match_id })
    }

    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "https://moenarch.com/native.html?lobbyId={}&matchId={}",
            self.lobby_id,
            self.match_id.as_str()
        )
    }
}

/// Proof is public key plus a signature bound to this authenticated signaling participant.
#[must_use]
pub fn admission_message(lobby: &str, participant: &str, matched: &str) -> Vec<u8> {
    format!("mmorpg-guest-v1\0{lobby}\0{participant}\0{matched}").into_bytes()
}

/// Production is fixed to the same-origin service. The opt-in real-process test may
/// address only a loopback HTTP service; this is never accepted from invitation text.
pub fn setup_config() -> Result<game_server::webrtc_transport::SetupConfig, String> {
    let endpoint = match std::env::var("MMORPG_TEST_SETUP_ENDPOINT") {
        Ok(value) => {
            let parsed = Url::parse(&value).map_err(|_| "Invalid loopback test setup endpoint")?;
            if parsed.scheme() != "http"
                || !matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            {
                return Err("Test setup endpoint must be loopback HTTP".into());
            }
            value
        }
        Err(std::env::VarError::NotPresent) => "https://moenarch.com".into(),
        Err(_) => return Err("Invalid test setup endpoint".into()),
    };
    game_server::webrtc_transport::SetupConfig::new(&endpoint, "https://moenarch.com")
        .map_err(|e| e.to_string())
}

pub fn bind_ip() -> Result<String, String> {
    if let Ok(value) = std::env::var("MMORPG_WEBRTC_BIND_IP") {
        let ip: std::net::IpAddr = value
            .parse()
            .map_err(|_| "Invalid WebRTC interface address")?;
        if ip.is_unspecified() || ip.is_multicast() {
            return Err("Choose an actual local WebRTC interface address".into());
        }
        return Ok(ip.to_string());
    }
    if std::env::var_os("MMORPG_TEST_SETUP_ENDPOINT").is_some() {
        return Ok("127.0.0.1".into());
    }
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket
        .connect("1.1.1.1:53")
        .map_err(|_| "No network route. Connect to a network and retry Host World.")?;
    Ok(socket
        .local_addr()
        .map_err(|e| e.to_string())?
        .ip()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_landing_contract_and_canonical_emission() {
        let url = "https://moenarch.com/native.html?lobbyId=0123456789AB&matchId=zone-1";
        assert_eq!(JoinInvite::parse(url).unwrap().url(), url);
        for suffix in [
            "&api=http://evil",
            "&matchId=other",
            "#secret",
            "&participantToken=secret",
        ] {
            assert!(JoinInvite::parse(&format!("{url}{suffix}")).is_err());
        }
        for bad in [
            url.replace("moenarch.com", "evil.example"),
            url.replace("0123456789AB", "0123456789ab"),
            url.replace("native.html", "index.html"),
        ] {
            assert!(JoinInvite::parse(&bad).is_err());
        }
    }
}
