// A configuration in JavaScript, which would switch the rule below off. The
// tool never reads it and Stylelint never loads it: the check hands Stylelint
// a configuration the tool built. Were it loaded, it would stop the check of
// every stylesheet with this error.
throw new Error("the configuration in JavaScript was loaded");

export default {
  rules: {
    "color-hex-length": null,
  },
};
