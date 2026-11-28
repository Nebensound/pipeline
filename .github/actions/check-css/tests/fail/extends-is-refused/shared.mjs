// A configuration the repository's file extends. Stylelint never loads it: the
// check refuses `extends` by name. Were it loaded, it would stop the check of
// every stylesheet with this error.
throw new Error("the configuration the repository's file extends was loaded");
