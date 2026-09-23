use std::io;
use std::process::ExitCode;

use congmiao_core::{
    data_dir, decode_frame, dispatch_host, parse_request, write_frame, DaemonClient, DaemonPaths,
    Endpoint, Error, HostResponse,
};

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    if let Err(err) = runtime.block_on(run()) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

async fn run() -> Result<(), Error> {
    loop {
        let payload = {
            let mut input = io::stdin().lock();
            match decode_frame(&mut input) {
                Ok(payload) => payload,
                Err(err) if is_eof(&err) => break,
                Err(err) => return Err(err),
            }
        };
        let response = match parse_request(&payload) {
            Ok(request) => match connect().await {
                Ok(client) => dispatch_host(request, &client).await,
                Err(err) => HostResponse::Error {
                    id: request_id(&payload),
                    message: err.to_string(),
                },
            },
            Err(err) => HostResponse::Error {
                id: "unknown".into(),
                message: err.to_string(),
            },
        };
        let mut output = io::stdout().lock();
        write_frame(&mut output, &response)?;
    }
    Ok(())
}

async fn connect() -> Result<DaemonClient, Error> {
    let paths = DaemonPaths::new(data_dir()?);
    let endpoint = Endpoint::load(&paths.endpoint())?;
    let client = DaemonClient::new(&endpoint)?;
    client.health().await?;
    Ok(client)
}

fn request_id(payload: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| value.get("id")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

fn is_eof(err: &Error) -> bool {
    matches!(
        err,
        Error::Io(message)
            if message.contains("unexpected end of file")
                || message.contains("failed to fill whole buffer")
    )
}
