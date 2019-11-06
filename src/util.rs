use futures::compat::Future01CompatExt;
use failure::Error;

pub async fn async_reqwest_to_error(request: reqwest::r#async::RequestBuilder) -> Result<reqwest::r#async::Response, Error> {
    let c = request.send().compat().await?;
    Ok(c.error_for_status()?)
}