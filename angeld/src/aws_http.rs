use aws_config::SdkConfig;
use aws_config::timeout::TimeoutConfig;
use aws_smithy_http_client::hyper_014::HyperClientBuilder;
use hyper_rustls::HttpsConnectorBuilder;

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
    let http_client = HyperClientBuilder::new().build(https_connector);

    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .http_client(http_client)
        .region(region)
        .timeout_config(timeout_config)
        .load()
        .await
}
