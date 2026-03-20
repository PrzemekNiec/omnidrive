use aws_config::SdkConfig;
use aws_config::timeout::TimeoutConfig;
use aws_smithy_http_client::hyper_014::HyperClientBuilder;
use hyper_rustls::HttpsConnectorBuilder;

pub async fn load_shared_config(
    region: aws_config::Region,
    timeout_config: TimeoutConfig,
) -> SdkConfig {
    let https_connector = HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_only()
        .enable_http1()
        .enable_http2()
        .build();
    let http_client = HyperClientBuilder::new().build(https_connector);

    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .http_client(http_client)
        .region(region)
        .timeout_config(timeout_config)
        .load()
        .await
}
