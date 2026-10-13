const assert = require("node:assert/strict");
const test = require("node:test");
const {
  oauthRoleOptionIndex,
  snowflakeOauthPageAction,
} = require("./oauthBrowserSteps.js");

test("recognizes the loopback success page", () => {
  assert.equal(
    snowflakeOauthPageAction(
      "Authorization Complete Authorization completed successfully. You may close this window.",
    ),
    "done",
  );
  assert.equal(
    snowflakeOauthPageAction(
      "Access to Snowflake has been granted to the ODBC driver.",
    ),
    "done",
  );
});

test("leaves a passkey prompt for the authenticator app", () => {
  assert.equal(
    snowflakeOauthPageAction(
      "Verify with passkey You will be prompted to use a security key Retry Back",
    ),
    "passkey-back",
  );
  assert.equal(
    snowflakeOauthPageAction("Set up passkey Follow the instructions Back"),
    "passkey-back",
  );
});

test("picks the authenticator from the MFA method list", () => {
  assert.equal(
    snowflakeOauthPageAction(
      "Select an MFA method to continue Passkey Recommended Authenticator Duo",
    ),
    "choose-authenticator",
  );
});

test("submits the authenticator code and the role picker", () => {
  assert.equal(
    snowflakeOauthPageAction(
      "Verify with your authenticator app Enter the 6 digit code Continue",
    ),
    "totp",
  );
  assert.equal(
    snowflakeOauthPageAction(
      "Choose a role for driver-oauth access PUBLIC Select Cancel",
    ),
    "select-role",
  );
});

test("allows the consent screen and surfaces an IdP error", () => {
  assert.equal(
    snowflakeOauthPageAction(
      "driver-oauth would like access to your Snowflake Account Allow Cancel",
    ),
    "allow",
  );
  assert.equal(
    snowflakeOauthPageAction("Authorization failed. You may close this window."),
    "idp-error",
  );
});

test("picks the PUBLIC role option, including one labeled with the Default badge", () => {
  assert.equal(oauthRoleOptionIndex(["SYSADMIN", "PUBLIC", "ACCOUNTADMIN"]), 1);
  assert.equal(oauthRoleOptionIndex(["PUBLIC Default", "ACCOUNTADMIN"]), 0);
  assert.equal(oauthRoleOptionIndex(["SYSADMIN", "ACCOUNTADMIN"]), 0);
  assert.equal(oauthRoleOptionIndex([]), -1);
});

test("waits while the next screen is still loading", () => {
  assert.equal(snowflakeOauthPageAction(""), "wait");
  assert.equal(snowflakeOauthPageAction("Sign in"), "wait");
});
