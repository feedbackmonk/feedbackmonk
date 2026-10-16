// uldf-template: testing-liveness v3
// Vendored from <home>/templates/testing/playwright/ (TLV-10). The config's `globalSetup`. A project
// with a globalSetup of its own lists both, this one first: `[queue-setup, its own]`.
import type { FullConfig } from "@playwright/test";
import { enterQueue } from "./queue";

export default async function queueSetup(config: FullConfig): Promise<void> {
  await enterQueue(config);
}
