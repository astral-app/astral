use super::host::{self, Reply, Request};

const METHODS: [&str; 5] = ["DELETE", "GET", "PATCH", "POST", "PUT"];

fn valid(method: &str, path: &str) -> bool {
    METHODS.contains(&method) && path.starts_with('/') && !path.starts_with("//") && !path.contains("..")
}

pub fn send(request: u64, method: &str, path: &str, body: &str) -> bool {
    valid(method, path)
        && matches!(
            host::request(Request::DiscordRequest {
                request,
                method,
                path,
                body,
            }),
            Reply::Done | Reply::Flag(true)
        )
}
