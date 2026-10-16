//! Decoding of Arrow record batches into JS row arrays.

mod column_reader;
mod column_source;
mod decfloat;
mod js_cell;
mod numeric;
mod stream_state;
mod temporal;
mod vector;

pub(crate) use stream_state::StreamState;
