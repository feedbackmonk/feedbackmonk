import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// No time limit stops a `waitFor` / `findBy*` (DEC-743): their default is 1 s,
// and `0` is not "none" there, so the largest timer value stands in for it.
configure({ asyncUtilTimeout: 2_147_483_647 });
