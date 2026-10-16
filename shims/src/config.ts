// The Session's bootstrap data, written by the dev server just before this
// bundle as `window.__TIZEN_SIM__`.

interface AppConfig {
  id: string;
  packageId: string;
  name: string;
  version: string;
  privileges: string[];
  // Milliseconds since the epoch; fixed for the Session so it doesn't change on reload.
  installedAt: number;
}

interface RemoteKeyConfig {
  name: string;
  code: number;
  mandatory: boolean;
}

interface ProfileConfig {
  version: string;
  milestone: number;
  capabilities: { [key: string]: boolean | number | string };
  device: { [key: string]: any };
  // Every Remote Key these TVs have, Mandatory and Registrable.
  remoteKeys: RemoteKeyConfig[];
  // API members these TVs lack or deprecate (see availability.ts).
  apis: { absent: string[]; deprecated: string[] };
  // The media formats these TVs play (see media.ts); null when unknown.
  media: MediaConfig | null;
  // Web features these TVs lack, and what of each to delete (see features.ts).
  webFeatures: { name: string; paths: string[] }[];
}

interface MediaConfig {
  year: number;
  // The specification page, under developer.samsung.com/smarttv/develop/.
  page: string;
  modelGroups: string[];
  formats: { containers: string[]; codecs: string[]; music: boolean }[];
  // A codec only some model groups list, with those groups.
  someModels: { [codec: string]: string[] };
}

interface SimConfig {
  bridgeUrl: string;
  // Where the dev server serves the files of the App's Sandbox.
  sandboxPath: string;
  lenient: boolean;
  // The App's screen size in CSS px.
  screen: { width: number; height: number };
  app: AppConfig;
  profile: ProfileConfig;
  // Keyboard keyCode -> Remote Key name, for keys with no meaning on a TV.
  keyMap: { [keyCode: string]: string };
  // Hooks the Host calls through CDP; installed by the Shims.
  keys?: { isRegistered: (name: string) => boolean };
  // Installs `webapis`; called by the virtual $WEBAPIS/webapis/webapis.js.
  installWebapis?: () => void;
}

var config: SimConfig = (window as any).__TIZEN_SIM__;
