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
}

interface SimConfig {
  bridgeUrl: string;
  lenient: boolean;
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
