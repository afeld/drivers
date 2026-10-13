// The SSO-first login layout renders "Sign in with username & password" as a
// disclosure ahead of the real submit button, whose accessible name is "Sign
// in". A substring match clicks the disclosure and collapses the form.

const SIGN_IN_SUBMIT = "Sign in";

function normalizeButtonName(name) {
  return String(name || "")
    .replace(/\s+/g, " ")
    .trim();
}

function matchingSignInSubmit(accessibleNames) {
  const exact = accessibleNames
    .map(normalizeButtonName)
    .filter((name) => name === SIGN_IN_SUBMIT);
  return exact.length === 1 ? SIGN_IN_SUBMIT : null;
}

// Clicking the disclosure toggles it. Expand only when the password field is
// not on the page. An already-open form, including a later visit that starts
// open, must not be toggled shut.
function shouldExpandUsernamePasswordDisclosure({
  ariaExpanded,
  passwordFieldVisible,
}) {
  if (passwordFieldVisible) {
    return false;
  }
  return ariaExpanded !== "true";
}

module.exports = {
  SIGN_IN_SUBMIT,
  USERNAME_PASSWORD_DISCLOSURE: "Sign in with username & password",
  matchingSignInSubmit,
  shouldExpandUsernamePasswordDisclosure,
};
