@nodejs
Feature: Connection configuration from TOML files

  @nodejs_e2e
  Scenario: should connect using the default profile when no connection options are given
    Given connections.toml defines a "default" profile with valid credentials
    When a connection is opened with no connection options
    Then SELECT 1 returns 1

  @nodejs_e2e
  Scenario: should honor SNOWFLAKE_DEFAULT_CONNECTION_NAME
    Given connections.toml defines a valid "fromFile" profile and an invalid "default" profile
    And SNOWFLAKE_DEFAULT_CONNECTION_NAME is "fromFile"
    When a connection is opened with no connection options
    Then SELECT 1 returns 1

  @nodejs_e2e
  Scenario: should connect a pool when no connection options are given
    Given connections.toml defines a "default" profile with valid credentials
    When a pool is opened with no connection options
    Then SELECT 1 returns 1

  @nodejs_e2e
  Scenario: should connect using a named profile from connections.toml
    Given connections.toml defines a "fromFile" profile with valid credentials
    When a connection is opened with connection name "fromFile"
    Then SELECT 1 returns 1

  @nodejs_e2e
  Scenario: should fall back to a profile defined only in config.toml
    Given config.toml names "fromFile" as the default and defines that profile
    And connections.toml does not exist
    When a connection is opened with no connection options
    Then SELECT 1 returns 1

  @nodejs_e2e
  Scenario: should let explicit options override the profile
    Given connections.toml defines a "fromFile" profile with an invalid account
    When a connection is opened with connection name "fromFile" and valid credentials
    Then SELECT 1 returns 1

  @nodejs_e2e
  Scenario: should fail when the named profile is missing
    Given connections.toml defines a "default" profile
    When a connection is opened with connection name "missing"
    Then the connection fails because the profile does not exist

  @nodejs_e2e
  Scenario: should reject a group-or-world-writable connections.toml
    Given connections.toml is writable by others
    When a connection is opened with no connection options
    Then the connection fails because the file permissions are unsafe
