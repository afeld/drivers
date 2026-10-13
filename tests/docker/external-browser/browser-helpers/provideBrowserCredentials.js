const assert = require("assert");
const { chromium } = require("playwright");
const {
  oauthRoleOptionIndex,
  snowflakeOauthPageAction,
} = require("./oauthBrowserSteps.js");
const {
  SIGN_IN_SUBMIT,
  USERNAME_PASSWORD_DISCLOSURE,
  matchingSignInSubmit,
  shouldExpandUsernamePasswordDisclosure,
} = require("./snowflakeSignIn.js");
const TotpGenerator = require("./totpGenerator.js");

const timeoutInMillis = 15000;

const connectToSnowflakeLogin = async () => {
  let page;
  const timeout = Date.now() + timeoutInMillis;
  while (Date.now() < timeout) {
    try {
      const browser = await chromium.connectOverCDP("http://localhost:9222");
      const defaultContext = browser.contexts()[0];
      page = defaultContext.pages()[0];
      const usernameVisible =
        (await page.locator('input[autocomplete="username"]').count()) > 0;
      const disclosureVisible =
        (await page
          .getByRole("button", {
            name: USERNAME_PASSWORD_DISCLOSURE,
            exact: true,
          })
          .count()) > 0;
      if (usernameVisible || disclosureVisible) {
        return page;
      }
    } catch (err) {
      //Wait for browser to be ready
    }
  }
  if (!page) {
    await assert.fail("Cannot connect to browser");
  }
  await assert.fail("Snowflake login page did not appear");
};

const connectToExternalBrowser = async (
  loginElementInput = 'input[type="submit"]',
  loginField = 'input[name="identifier"]',
) => {
  let page;
  const timeout = Date.now() + timeoutInMillis;
  while (Date.now() < timeout) {
    try {
      const browser = await chromium.connectOverCDP("http://localhost:9222");
      const defaultContext = await browser.contexts()[0];
      page = await defaultContext.pages()[0];
      await page.waitForSelector(loginElementInput, { timeout: 500 });
      await page.waitForSelector(loginField, { timeout: 500 });
      return page;
    } catch (err) {
      //Wait for browser to be ready
    }
  }
  if (!page) {
    await assert.fail("Cannot connect to browser");
  }
};

const fillExternalBrowserCredentialsInSnowflake = async (
  page,
  login,
  password,
  loginElementInput = 'input[name="identifier"]',
  credentialElementInput = 'input[name="credentials.passcode"]',
) => {
  const timeout = Date.now() + timeoutInMillis;

  while (Date.now() < timeout) {
    try {
      let providedLogin = await page.$eval(loginElementInput, (el) => el.value);
      let providedPassword = await page.$eval(
        credentialElementInput,
        (el) => el.value,
      );
      if (providedLogin !== login) {
        await page.fill(loginElementInput, login);
        providedLogin = await page.$eval(loginElementInput, (el) => el.value);
      }
      if (providedPassword !== password) {
        await page.fill(credentialElementInput, password);
        providedPassword = await page.$eval(
          credentialElementInput,
          (el) => el.value,
        );
      }
      if (providedLogin === login && providedPassword === password) {
        return true;
      }
    } catch (err) {
      //Wait for browser to be ready
    }
  }
  return false;
};

const fillExternalBrowserCredentialsInOkta = async (
  page,
  login,
  password,
  loginElementInput = 'input[name="identifier"]',
  credentialElementInput = 'input[name="credentials.passcode"]',
) => {
  const timeout = Date.now() + timeoutInMillis;

  while (Date.now() < timeout) {
    try {
      // Step 1: Fill login field
      let providedLogin = await page.$eval(loginElementInput, (el) => el.value);
      if (providedLogin !== login) {
        await page.fill(loginElementInput, login);
        providedLogin = await page.$eval(loginElementInput, (el) => el.value);
      }

      // Step 2: Click next button if login is filled correctly
      if (providedLogin === login) {
        await page.click('input[value="Next"]');

        // Step 3: Wait for password field to be available and fill it
        await page.waitForSelector(credentialElementInput, { timeout: 5000 });

        let providedPassword = await page.$eval(
          credentialElementInput,
          (el) => el.value,
        );
        if (providedPassword !== password) {
          await page.fill(credentialElementInput, password);
          providedPassword = await page.$eval(
            credentialElementInput,
            (el) => el.value,
          );
        }

        // Step 4: Check if both login and password are correctly filled
        if (providedLogin === login && providedPassword === password) {
          return true;
        }
      }
    } catch (err) {
      //Wait for browser to be ready or for elements to appear
    }
  }
  return false;
};

const submitAndCheckOktaErrorMessage = async (page) => {
  let errorMessage = "";
  await page.click('input[type="submit"]');
  try {
    await page.waitForSelector(
      ".okta-form-infobox-error.infobox.infobox-error",
      { timeout: 5000 },
    );
    errorMessage = await page.$eval(
      ".okta-form-infobox-error.infobox.infobox-error p",
      (el) => el.textContent,
    );
  } catch (err) {
    //No 'login error message'
  }
  if (errorMessage) {
    return errorMessage;
  } else {
    return "";
  }
};

const ensureUsernamePasswordFormExpanded = async (page) => {
  const disclosure = page.getByRole("button", {
    name: USERNAME_PASSWORD_DISCLOSURE,
    exact: true,
  });
  if ((await disclosure.count()) === 0) {
    return;
  }
  const ariaExpanded = await disclosure.first().getAttribute("aria-expanded");
  const passwordFieldVisible =
    (await page.locator('input[autocomplete="current-password"]').count()) > 0;
  if (
    shouldExpandUsernamePasswordDisclosure({
      ariaExpanded,
      passwordFieldVisible,
    })
  ) {
    await disclosure.first().click();
    await page
      .locator('input[autocomplete="current-password"]')
      .waitFor({ timeout: 5000 });
  }
};

const submitAndCheckSnowflakeErrorMessage = async (page) => {
  let errorMessage = "";
  await ensureUsernamePasswordFormExpanded(page);
  const buttonNames = await page.getByRole("button").allInnerTexts();
  const submitName = matchingSignInSubmit(buttonNames) || SIGN_IN_SUBMIT;
  await page.getByRole("button", { name: submitName, exact: true }).click();

  try {
    await page.waitForSelector('div[role="status"]', { timeout: 1000 });
    errorMessage = await page.$eval(
      'div[role="status"]',
      (el) => el.textContent,
    );
  } catch (err) {
    //No 'login error message'
  }
  if (errorMessage) {
    return errorMessage;
  } else {
    return "";
  }
};

const getExternalBrowserOktaResponse = async (page) => {
  return await page.waitForSelector("text=Your identity was confirmed", {
    timeout: 10000,
  });
};

// Fill Snowflake's authenticator-app MFA verification step, if presented. No-op if the
// field never appears, so this stays safe to call for accounts that don't require MFA.
const fillMfaCodeIfPresented = async (page, totpSeed, deadline) => {
  if (!totpSeed) {
    return;
  }
  const mfaCodeInput = 'input[autocomplete="one-time-code"]';
  try {
    await page.waitForSelector(mfaCodeInput, { timeout: 5000 });
  } catch (err) {
    return;
  }
  const totp = new TotpGenerator(totpSeed);
  // Jenkins already serializes Python/ODBC/JDBC under
  // drivers-auth-browser-shared-mfa-accounts (#1329). Do not re-enable
  // parallel stages on this shared seed without per-language users
  // (SNOW-4017658). Retry with a fresh window if this attempt is rejected.
  const maxAttempts = 2;
  const totpStepMs = 30_000;
  const detachTimeoutMs = 8000;
  for (let attempt = 0; attempt < maxAttempts; attempt++) {
    // generateTotp waits out a near-boundary window (MIN_VALIDITY_SECONDS)
    // so the code stays valid through fill + click + redirect latency.
    const current = await totp.generateTotp();
    const submittedWindow = Math.floor(Date.now() / totpStepMs);
    await page.fill(mfaCodeInput, current);
    await page.click('button:has-text("Continue")');
    try {
      // Success navigates away, detaching this input. A rejected code leaves it
      // attached (e.g. still showing an "invalid code" message) - retry in that case.
      await page.waitForSelector(mfaCodeInput, {
        state: "detached",
        timeout: detachTimeoutMs,
      });
      return;
    } catch (err) {
      // Only wait out this window if we are still in it — a detach poll that
      // already crossed the boundary must not burn the next unused window.
      if (Math.floor(Date.now() / totpStepMs) !== submittedWindow) {
        continue;
      }
      const waitMs = totp.getTimeRemainingMs() + 1000;
      // A second attempt is only worth starting if the outer walk's deadline
      // leaves room for the wait plus the retry itself.
      if (deadline !== undefined && Date.now() + waitMs >= deadline) {
        break;
      }
      await totp.sleep(waitMs);
    }
  }
  throw new Error(`MFA verification failed after ${maxAttempts} attempts`);
};

const pageText = async (page) => {
  try {
    return await page.locator("body").innerText({ timeout: 1000 });
  } catch (err) {
    return "";
  }
};

const clickExactButton = async (page, name) => {
  const target = page.getByRole("button", { name, exact: true });
  if ((await target.count()) === 0) {
    return false;
  }
  try {
    await target.first().click({ timeout: 3000 });
    return true;
  } catch (err) {
    return false;
  }
};

const selectOauthRole = async (page) => {
  const options = page
    .getByRole("listbox", { name: "Available roles" })
    .getByRole("option");
  const count = await options.count();
  const names = [];
  for (let i = 0; i < count; i++) {
    names.push(await options.nth(i).innerText());
  }
  const index = oauthRoleOptionIndex(names);
  if (index < 0) {
    return;
  }
  try {
    await options.nth(index).click({ timeout: 3000 });
  } catch (err) {
    //Role already selected, or not clickable yet
  }
};

// Walk the screens between password submit and the loopback success page:
// passkey prompt, MFA method list, authenticator code, role picker, and the
// Allow consent screen.
const checkOauthResponse = async (page, totpSeed) => {
  const deadline = Date.now() + 45_000;
  let mfaAttempted = false;
  while (Date.now() < deadline) {
    const text = await pageText(page);
    const action = snowflakeOauthPageAction(text);
    if (action === "done") {
      return;
    }
    if (action === "idp-error") {
      throw new Error(
        `Providing credentials was not successful (${oauthPageSummary(page, text)})`,
      );
    }
    if (action === "totp") {
      if (!mfaAttempted) {
        mfaAttempted = true;
        await fillMfaCodeIfPresented(page, totpSeed, deadline);
      }
    } else if (action === "passkey-back") {
      await clickExactButton(page, "Back");
    } else if (action === "choose-authenticator") {
      await clickExactButton(page, "Authenticator");
    } else if (action === "select-role") {
      await selectOauthRole(page);
      await clickExactButton(page, "Select");
    } else if (action === "allow") {
      await clickExactButton(page, "Allow");
    }
    await page.waitForTimeout(250);
  }
  throw new Error(
    `Providing credentials was not successful (${oauthPageSummary(page, await pageText(page))})`,
  );
};

const oauthPageSummary = (page, text) => {
  let url = "";
  try {
    url = page.url();
  } catch (err) {
    url = "";
  }
  const snippet = String(text || "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 400);
  return `url=${url} page=${JSON.stringify(snippet)}`;
};

const closeBrowser = async (page) => {
  await page.close();
};

const successfulExternalBrowserOktaConnection = async (login, password) => {
  const page = await connectToExternalBrowser();
  await assert.notEqual(page, null, "Browser connection timed out");

  const credentialsProvided = await fillExternalBrowserCredentialsInOkta(
    page,
    login,
    password,
  );
  await assert.equal(credentialsProvided, true, "Cannot provided credentials");

  const errMsg = await submitAndCheckOktaErrorMessage(page);
  await assert.equal(errMsg, "", errMsg);

  const response = await getExternalBrowserOktaResponse(page);
  await assert.ok(response, "Your identity was confirmed");
  await closeBrowser(page);
};

const failedExternalBrowserOktaConnection = async (login, password) => {
  const page = await connectToExternalBrowser();
  await assert.notEqual(page, null, "Browser connection timed out");

  const credentialsProvided = await fillExternalBrowserCredentialsInOkta(
    page,
    login,
    password,
  );
  await assert.equal(credentialsProvided, true, "Cannot provided credentials");

  const errMsg = await submitAndCheckOktaErrorMessage(page);
  await assert.equal(errMsg, "Unable to sign in", errMsg);

  await closeBrowser(page);
};

const successfulExternalOauthOktaConnection = async (login, password) => {
  const page = await connectToExternalBrowser();
  await assert.notEqual(page, null, "Browser connection timed out");

  const credentialsProvided = await fillExternalBrowserCredentialsInOkta(
    page,
    login,
    password,
  );
  await assert.equal(credentialsProvided, true, "Cannot provided credentials");

  const errMsg = await submitAndCheckOktaErrorMessage(page);
  await assert.equal(errMsg, "", errMsg);
  await checkOauthResponse(page);

  await closeBrowser(page);
};

const successfulInternalOauthSnowflakeConnection = async (
  login,
  password,
  totpSeed,
) => {
  const loginElementInput = 'input[autocomplete="username"]';
  const credentialsElementInput = 'input[autocomplete="current-password"]';

  const page = await connectToSnowflakeLogin();
  await assert.notEqual(page, null, "Browser connection timed out");
  await ensureUsernamePasswordFormExpanded(page);

  const credentialsProvided = await fillExternalBrowserCredentialsInSnowflake(
    page,
    login,
    password,
    loginElementInput,
    credentialsElementInput,
  );
  await assert.equal(credentialsProvided, true, "Cannot provided credentials");

  const errMsg = await submitAndCheckSnowflakeErrorMessage(page);
  await assert.equal(errMsg, "", errMsg);
  await checkOauthResponse(page, totpSeed);

  await closeBrowser(page);
};

const args = process.argv.slice(2);
async function main() {
  if (args[0] === "success") {
    await successfulExternalBrowserOktaConnection(args[1], args[2]);
  } else if (args[0] === "fail") {
    await failedExternalBrowserOktaConnection(args[1], args[2]);
  } else if (args[0] === "timeout") {
    await connectToExternalBrowser();
  } else if (args[0] === "externalOauthOktaSuccess") {
    await successfulExternalOauthOktaConnection(args[1], args[2]);
  } else if (args[0] === "internalOauthSnowflakeSuccess") {
    await successfulInternalOauthSnowflakeConnection(args[1], args[2], args[3]);
  }
  process.exit(0);
}

main();
