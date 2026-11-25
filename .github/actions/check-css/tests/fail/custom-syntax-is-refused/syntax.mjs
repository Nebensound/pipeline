// A syntax the repository's file names. Stylelint never loads it: the check
// refuses `customSyntax` by name. Were it loaded, it would stop the check of
// every stylesheet with this error.
throw new Error("the syntax the repository's file names was loaded");
