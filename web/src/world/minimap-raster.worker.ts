import { runMinimapRaster, type MinimapRasterRequest } from "./minimap-raster";

/** Rasterizes one full map away from the page (#527); the page starts a worker per map and ends it after the answer. */
const scope = self as unknown as {
  onmessage: ((event: MessageEvent<MinimapRasterRequest>) => void) | null;
  postMessage(message: unknown, transfer: Transferable[]): void;
};

scope.onmessage = ({ data }) => {
  const { result, transfer } = runMinimapRaster(data);
  scope.postMessage(result, transfer);
};
