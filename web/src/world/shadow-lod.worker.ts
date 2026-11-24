import { indexLevels, simplifierReady, type ShadowBakeAnswer, type ShadowBakeRequest } from "./shadow-lod-indices";

/** Simplifies unit shadow proxies away from the page (#1122); `shadow-bake.ts` starts one and keeps it. */
const scope = self as unknown as {
  onmessage: ((event: MessageEvent<ShadowBakeRequest>) => void) | null;
  postMessage(message: ShadowBakeAnswer, transfer: Transferable[]): void;
};

scope.onmessage = ({ data }) => {
  void simplifierReady.then(() => {
    const levels = indexLevels(data.indices, data.positions, data.levels);
    scope.postMessage({ id: data.id, levels }, levels.flatMap((level) => (level ? [level.buffer] : [])));
  }).catch((error: unknown) => {
    scope.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) }, []);
  });
};
