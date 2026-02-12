use anyhow::{Context, Result};
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    sign, SignableBody, SignableRequest, SignatureLocation, SigningSettings,
};
use aws_sigv4::sign::v4::SigningParams;
use std::time::SystemTime;

/// Sign an HTTP request using AWS SigV4 for Bedrock.
/// Returns the signed headers as (name, value) pairs.
///
/// Accepts headers as `&str` slices to avoid per-request String allocation
/// for static header values like "content-type" and "host".
pub fn sign_request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    credentials: &Credentials,
    region: &str,
) -> Result<Vec<(String, String)>> {
    let mut settings = SigningSettings::default();
    settings.signature_location = SignatureLocation::Headers;

    let identity = credentials.clone().into();

    let signing_params = SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("bedrock")
        .time(SystemTime::now())
        .settings(settings)
        .build()
        .context("Failed to build SigV4 signing params")?;

    let signable_request = SignableRequest::new(
        method,
        url,
        headers.iter().map(|(k, v)| (*k, *v)),
        SignableBody::Bytes(body),
    )
    .context("Failed to create signable request")?;

    let (signing_instructions, _signature) =
        sign(signable_request, &signing_params.into())
            .context("Failed to sign request")?
            .into_parts();

    // Build an http::Request to apply signing instructions to
    let mut builder = http::Request::builder()
        .method(method)
        .uri(url);

    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }

    let mut request = builder
        .body(())
        .context("Failed to build HTTP request for signing")?;

    signing_instructions.apply_to_request_http1x(&mut request);

    // Extract all headers from the signed request
    let signed_headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect();

    Ok(signed_headers)
}
