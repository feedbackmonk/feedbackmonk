// uldf-template: testing-liveness v1
// Vendored from <home>/templates/testing/playwright/ (TLV-10). The config's `globalSetup`.
import type { FullConfig } from "@playwright/test";
import { enterQueue } from "./queue";

export default function queueSetup(config: FullConfig): void {
  enterQueue(config);
}
