// Post-password screens the Snowflake login UI can insert before the
// loopback success page. The helper used to wait only for the TOTP field
// and an Allow button, so a method picker, passkey prompt, or role picker
// stalled the authorization-code flow.

const SUCCESS_SNIPPETS = [
  "Authorization completed successfully",
  "Access to Snowflake has been granted to the ODBC driver",
  "OAuth authentication completed successfully",
  "Your identity was confirmed and propagated to Snowflake",
];

function snowflakeOauthPageAction(visibleText) {
  const text = String(visibleText || "")
    .replace(/\s+/g, " ")
    .trim();
  if (SUCCESS_SNIPPETS.some((snippet) => text.includes(snippet))) {
    return "done";
  }
  if (text.includes("Authorization failed")) {
    return "idp-error";
  }
  if (
    text.includes("Verify with your authenticator app") ||
    text.includes("Enter the 6 digit code")
  ) {
    return "totp";
  }
  if (text.includes("Verify with passkey") || text.includes("Set up passkey")) {
    return "passkey-back";
  }
  if (text.includes("Select an MFA method")) {
    return "choose-authenticator";
  }
  if (text.includes("Choose a role")) {
    return "select-role";
  }
  if (text.includes("would like access to your Snowflake Account")) {
    return "allow";
  }
  return "wait";
}

function normalizeOptionName(name) {
  return String(name || "")
    .replace(/\s+/g, " ")
    .trim();
}

// Role rows are options, not radios. The default badge sits in the same
// option, so the visible name can be "PUBLIC Default".
function oauthRoleOptionIndex(optionNames) {
  const names = optionNames.map(normalizeOptionName);
  const preferred = names.findIndex(
    (name) => name === "PUBLIC" || name.startsWith("PUBLIC "),
  );
  if (preferred >= 0) {
    return preferred;
  }
  return names.findIndex((name) => name.length > 0);
}

module.exports = { oauthRoleOptionIndex, snowflakeOauthPageAction };
