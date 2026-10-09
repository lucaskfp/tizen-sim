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

interface ProfileConfig {
  version: string;
  milestone: number;
  capabilities: { [key: string]: boolean | number | string };
  device: { [key: string]: any };
}

interface SimConfig {
  bridgeUrl: string;
  lenient: boolean;
  app: AppConfig;
  profile: ProfileConfig;
}

var config: SimConfig = (window as any).__TIZEN_SIM__;
