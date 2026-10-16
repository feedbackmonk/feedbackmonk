// uldf-template: testing-liveness v1
// Vendored from <home>/templates/testing/playwright/ (TLV-10). The config's `globalTeardown`.
import { leaveQueue } from "./queue";

export default function queueTeardown(): void {
  leaveQueue();
}
