# How a customer checks they loaded the FIPS driver

Ship the same no-connection status call on the standard driver and on the
`fips` driver. The customer calls it at startup and checks that `is_fips`
returns true. A standard driver returns false, so loading the wrong binary
fails the check instead of missing the symbol.

The call does not open a connection and does not use the network.

## The single field

| Field | True means |
| --- | --- |
| `is_fips` | This process is running the FIPS driver and its TLS provider is in FIPS mode. |

On a build produced with `--features fips`, `is_fips` returns true. That is
what the FIPS smoke test asserts. On a normal build, it returns false.

## How to call it

```text
Python   snowflake.connector.get_fips_status()
         .is_fips

JDBC     new SnowflakeDriver().getFipsStatus()
         .isFips()

Node.js  getFipsStatus()
         .isFips

.NET     new SnowflakeDbDriver().GetFipsStatus()
         .IsFips

ODBC     SFGetFipsStatus(&status) == 0
         status.is_fips == 1
```

Python, as the customer would write it:

```python
status = snowflake.connector.get_fips_status()
if not status.is_fips:
    raise RuntimeError("Snowflake driver is not the FIPS build")
```

The same check in the other wrappers compares the field to true, or to `1`
for ODBC. Call it once at process start, before opening a connection.

## What the customer has checked

A true value means this process loaded the FIPS build and the TLS provider
inside it is the FIPS module. A false value means they loaded the standard
build. That is the distinction the two builds exist to give them.

The call does not certify the rest of the deployment. SigV4 request signing
and AES-ECB stage key wrap stay outside this result, and the response does
not name a certificate or a module version. Document the check as "this
process is the FIPS driver," not as "this driver is FIPS validated."

## Potential future extensions

Future extensions to `getFipsStatus` may include additional details such as
crypto module providers, their specific versions, and CMVP certificate numbers.
