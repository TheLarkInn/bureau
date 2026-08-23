//! GitHub rate-limit response classification.

use super::Error;

fn header_u64(response: &reqwest::Response, name: &str) -> Option<u64> {
    response.headers().get(name)?.to_str().ok()?.parse().ok()
}

fn retry_after(response: &reqwest::Response) -> Option<u64> {
    if let Some(seconds) = header_u64(response, "retry-after") {
        return Some(seconds);
    }
    let reset = header_u64(response, "x-ratelimit-reset")?;
    let clock = std::time::SystemTime::now;
    let now = clock()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(reset.saturating_sub(now))
}

fn exhausted(response: &reqwest::Response) -> bool {
    response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        || (response.status() == reqwest::StatusCode::FORBIDDEN
            && (header_u64(response, "x-ratelimit-remaining") == Some(0)
                || response.headers().contains_key("retry-after")))
}

fn secondary_limit(status: u16, message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    status == 403
        && (message.contains("secondary rate limit")
            || message.contains("abuse detection mechanism"))
}

pub async fn response_error(response: reqwest::Response) -> Result<Error, reqwest::Error> {
    let header_limited = exhausted(&response);
    let retry_after_secs = retry_after(&response);
    let status = response.status().as_u16();
    let bytes = response.bytes().await?;
    let message: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
    let limited = header_limited || secondary_limit(status, &message);
    if limited {
        Ok(Error::RateLimited {
            retry_after_secs,
            message,
        })
    } else {
        Ok(Error::Api { status, message })
    }
}
