use snafu::{Location, Snafu};

#[derive(Debug, Snafu, error_trace::ErrorTrace)]
#[snafu(visibility(pub(crate)))]
pub enum LogError {
    #[snafu(display("Failed to initialize logging: {message}"))]
    Init {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    #[snafu(display("Failed to reload logging layer"))]
    Reload {
        source: tracing_subscriber::reload::Error,
        #[snafu(implicit)]
        location: Location,
    },
}
