import "@testing-library/jest-dom/vitest";
import { clearMocks } from "@tauri-apps/api/mocks";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(async () => {
  cleanup();
  // Unmounting stops event listeners asynchronously, through the mocks.
  await new Promise((resolve) => setTimeout(resolve));
  clearMocks();
});
