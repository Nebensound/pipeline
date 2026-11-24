// Would switch every rule off and write a file into the repository, if it
// were loaded. markdownlint never loads it, so the README below still fails
// the rule the module would switch off.
import fs from "node:fs";

fs.writeFileSync("loaded-by-markdownlint", "this module was run\n");

export default { config: { default: false } };
