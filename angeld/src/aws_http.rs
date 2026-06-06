use aws_config::SdkConfig;
use aws_config::timeout::TimeoutConfig;
use aws_smithy_http_client::hyper_014::HyperClientBuilder;
use aws_smithy_types::retry::RetryConfig;
use hyper_rustls::HttpsConnectorBuilder;
use std::time::Duration;

const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn load_shared_config(
    region: aws_config::Region,
    timeout_config: TimeoutConfig,
    allow_http: bool,
) -> SdkConfig {
    let https_connector = if allow_http {
        HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build()
    } else {
        HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1()
            .build()
    };
    let hyper_builder = hyper::Client::builder()
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .clone();
    let http_client = HyperClientBuilder::new()
        .hyper_builder(hyper_builder)
        .build(https_connector);

    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .http_client(http_client)
        .region(region)
        .timeout_config(timeout_config)
        .retry_config(RetryConfig::adaptive())
        .load()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_config::Region;

    #[tokio::test]
    async fn shared_config_carries_retry_config() {
        let cfg =
            load_shared_config(Region::new("auto"), TimeoutConfig::builder().build(), true).await;
        assert!(cfg.retry_config().is_some());
    }
}
