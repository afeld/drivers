const assert = require("node:assert/strict");
const test = require("node:test");
const {
  matchingSignInSubmit,
  shouldExpandUsernamePasswordDisclosure,
} = require("./snowflakeSignIn.js");

test("substring Sign in hits the disclosure, the submit name does not", () => {
  const domOrder = ["Sign in with username & password", "Sign in"];
  const substringHit = domOrder.find((name) => name.includes("Sign in"));
  assert.equal(substringHit, "Sign in with username & password");
  assert.equal(matchingSignInSubmit(domOrder), "Sign in");
});

test("a collapsed disclosure has no submit button yet", () => {
  assert.equal(
    matchingSignInSubmit(["Sign in with username & password"]),
    null,
  );
});

test("the old layout's only Sign in button is the submit", () => {
  assert.equal(matchingSignInSubmit(["Sign in"]), "Sign in");
});

test("expands the username form only while the password field is hidden", () => {
  assert.equal(
    shouldExpandUsernamePasswordDisclosure({
      ariaExpanded: "false",
      passwordFieldVisible: false,
    }),
    true,
  );
  assert.equal(
    shouldExpandUsernamePasswordDisclosure({
      ariaExpanded: "true",
      passwordFieldVisible: true,
    }),
    false,
  );
  assert.equal(
    shouldExpandUsernamePasswordDisclosure({
      ariaExpanded: "false",
      passwordFieldVisible: true,
    }),
    false,
  );
});
