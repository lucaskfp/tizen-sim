// The TV's display and AV settings the platform APIs report (webapis.avinfo,
// tizen.tvdisplaycontrol): the Version Profile's values, which the developer
// can change. A change restarts the App with it.
import { useEffect, useState } from "react";
import * as commands from "@/lib/commands";

// EnergySavingType and EcoSensorType (avinfo-api.html).
const ENERGY_SAVING = ["OFF", "LOW", "MEDIUM", "HIGH"];
const ECO_SENSOR = ["OFF", "ON"];
// Display3DModeState and Display3DEffectMode (tvdisplaycontrol.html).
const MODE_3D_STATES = ["NOT_SUPPORTED", "NOT_CONNECTED", "READY"];
const EFFECTS_3D = [
  "OFF",
  "TOP_BOTTOM",
  "SIDE_BY_SIDE",
  "LINE_BY_LINE",
  "VERTICAL_STRIPE",
  "FRAME_SEQUENCE",
  "CHECKER_BD",
  "FROM_2D_TO_3D",
];

const selectStyle = "h-8 rounded-md border bg-background px-2 text-sm";

export function TvSettings({
  profile,
  adopted,
  onChange,
  onError,
}: {
  /** The Version Profile the App runs on; null before the Session says. */
  profile: string | null;
  /** Whether another process started the Session; that process chose its values. */
  adopted: boolean;
  onChange: (name: string, value: unknown) => void;
  onError: (e: unknown) => void;
}) {
  const [values, setValues] = useState<Record<string, unknown> | null>(null);

  useEffect(() => {
    setValues(null);
    if (profile !== null && !adopted) commands.deviceValues(profile).then(setValues, onError);
  }, [profile, adopted, onError]);

  if (adopted) {
    return (
      <p className="p-6 text-center text-sm text-muted-foreground">
        The process that started this Session chose its TV settings.
      </p>
    );
  }
  if (values === null) {
    return <p className="p-6 text-center text-sm text-muted-foreground">The TV's settings show once an App is open.</p>;
  }

  const change = (name: string, value: unknown) => {
    setValues({ ...values, [name]: value });
    onChange(name, value);
  };

  const choice = (name: string, label: string, options: string[]) => (
    <label className="flex items-center justify-between gap-2">
      {label}
      <select
        aria-label={label}
        className={selectStyle}
        value={String(values[name])}
        onChange={(e) => change(name, e.target.value)}
      >
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </label>
  );

  const supported = (values.supported3DEffectModes as string[] | undefined) ?? [];
  const toggleEffect = (effect: string, on: boolean) =>
    change(
      "supported3DEffectModes",
      EFFECTS_3D.filter((e) => (e === effect ? on : supported.includes(e))),
    );

  return (
    <div className="h-full space-y-4 overflow-auto p-3 text-sm">
      <p className="text-muted-foreground">Changing a setting restarts the App with it.</p>
      <section className="space-y-2">
        <h3 className="font-medium">Picture and sound (webapis.avinfo)</h3>
        <label className="flex items-center justify-between gap-2">
          HDR support
          <input
            type="checkbox"
            aria-label="HDR support"
            checked={values.hdrTvSupport === true}
            onChange={(e) => change("hdrTvSupport", e.target.checked)}
          />
        </label>
        {choice("energySaving", "Energy saving", ENERGY_SAVING)}
        {choice("ecoSensor", "Eco sensor", ECO_SENSOR)}
        <p className="text-xs text-muted-foreground">Apps read energy saving and the eco sensor from Tizen 6.5.</p>
      </section>
      <section className="space-y-2">
        <h3 className="font-medium">3D (tizen.tvdisplaycontrol)</h3>
        {choice("display3DModeState", "3D mode", MODE_3D_STATES)}
        {choice("display3DEffectMode", "3D effect", EFFECTS_3D)}
        <fieldset className="space-y-1">
          <legend className="mb-1">Supported 3D effects</legend>
          {EFFECTS_3D.map((effect) => (
            <label key={effect} className="flex items-center gap-2 font-mono text-xs">
              <input
                type="checkbox"
                aria-label={effect}
                checked={supported.includes(effect)}
                onChange={(e) => toggleEffect(effect, e.target.checked)}
              />
              {effect}
            </label>
          ))}
        </fieldset>
      </section>
    </div>
  );
}
