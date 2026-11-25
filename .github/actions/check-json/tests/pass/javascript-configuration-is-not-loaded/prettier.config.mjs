// Would write a file into the repository and stop Prettier with an error, if
// it were loaded. Prettier never loads it, so the case passes.
import fs from "node:fs";

fs.writeFileSync("loaded-by-prettier", "this module was run\n");
throw new Error("prettier.config.mjs was loaded");
