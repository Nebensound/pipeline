// A rule that would write a file into the repository, if it were loaded.
import fs from "node:fs";

fs.writeFileSync("loaded-by-markdownlint", "this module was run\n");

export default {
  names: ["loaded"],
  description: "the module was loaded",
  tags: ["test"],
  function: () => {},
};
