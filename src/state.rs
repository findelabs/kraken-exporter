use clap::ArgMatches;
use std::error::Error;
use hyper::{Body, Request};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::https::{HttpsClient, ClientBuilder};
use crate::error::Error as RestError;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const ASSET_PAIRS: &str = "https://api.kraken.com/0/public/AssetPairs";

#[derive(Clone, Debug)]
pub struct State {
    pub client: HttpsClient,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetPairs {
  error: Vec<String>,
  result: HashMap<String, Asset>
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
  wsname: String,
  base: String,
  quote: String
}

impl State {
    pub async fn new(opts: ArgMatches) -> BoxResult<Self> {
        // Set timeout
        let timeout: u64 = opts
            .value_of("timeout")
            .unwrap()
            .parse()
            .unwrap_or_else(|_| {
                eprintln!("Supplied timeout not in range, defaulting to 60");
                60
            });

        let client = ClientBuilder::new().timeout(timeout).build()?;

        Ok(State {
            client,
        })
    }

    pub async fn generate(&self) -> Result<(), RestError> {

        let mut req = Request::builder()
            .method("GET")
            .uri(ASSET_PAIRS)
            .body(Body::empty())
            .expect("request builder");

        let response = self.client.clone().request(req).await?;

        let body: AssetPairs = match response.status().as_u16() {
            404 => return Err(RestError::NotFound),
            403 => return Err(RestError::Forbidden),
            401 => return Err(RestError::Unauthorized),
            200 => {
                let contents = hyper::body::to_bytes(response.into_body()).await?;
                let body = serde_json::from_slice(&contents)?;
                body
            }
            _ => {
                log::error!(
                    "Got bad status code getting config: {}",
                    response.status().as_u16()
                );
                return Err(RestError::Unknown)
            }
        };

        log::debug!("{:?}", body);

        Ok(())
    }
}
