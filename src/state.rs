use clap::ArgMatches;
use std::error::Error;
use hyper::{Body, Request};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use hyper::body::Bytes;
use metrics::gauge;

use crate::https::{HttpsClient, ClientBuilder};
use crate::error::Error as RestError;

type BoxResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const ASSET_PAIRS: &str = "https://api.kraken.com/0/public/AssetPairs";
const TICKER: &str = "https://api.kraken.com/0/public/Ticker";

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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tickers {
  error: Vec<String>,
  result: HashMap<String, Info>
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Info {
  c: Vec<String>,
  v: Vec<String>,
  p: Vec<String>,
  t: Vec<u32>
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

    pub async fn get(&self, url: &str) -> Result<Bytes, RestError> {

        let mut req = Request::builder()
            .method("GET")
            .uri(url)
            .body(Body::empty())
            .expect("request builder");

        let response = self.client.clone().request(req).await?;

        match response.status().as_u16() {
            404 => return Err(RestError::NotFound),
            403 => return Err(RestError::Forbidden),
            401 => return Err(RestError::Unauthorized),
            200 => {
                Ok(hyper::body::to_bytes(response.into_body()).await?)
            }
            _ => {
                log::error!(
                    "Got bad status code getting config: {}",
                    response.status().as_u16()
                );
                return Err(RestError::Unknown)
            }
        }
    }

//  c: Vec<String>,
//  v: Vec<String>,
//  p: Vec<String>,
//  t: Vec<u32>

    pub async fn generate(&self) -> Result<(), RestError> {
        let bytes = self.get(ASSET_PAIRS).await?;
        let asset_pairs: AssetPairs = serde_json::from_slice(&bytes)?;

        log::debug!("{:?}", asset_pairs);

        let assets: Vec<String> = asset_pairs.result.iter().map(|(k,_)| k.to_string()).collect();
        let assets_query = assets.join(",");

        log::debug!("{:#?}", assets_query);

        let url = format!("{}?pair={}", TICKER, assets_query);
        log::debug!("url: {}", url);
        let bytes = self.get(&url).await?;
        let tickers: Tickers = serde_json::from_slice(&bytes)?;

        for (asset, value) in tickers.result.iter() {
            let asset_pair = &asset_pairs.result.get(asset).unwrap();
            let mut reference_currency = asset_pair.quote.to_string();
            reference_currency.remove(0);
            let labels = [
                ("currency", asset_pair.base.to_string()),
                ("reference_currency", reference_currency.to_string()),
                ("pair", asset_pair.wsname.to_string())
            ];
            gauge!("exchange_rate", value.c[0].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_volume", value.v[0].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_volume_last_day", value.v[1].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_rate_averate", value.p[0].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_rate_average_last_day", value.p[1].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_trades", value.t[0] as f64, &labels);
            gauge!("exchange_trades_last_day", value.t[1] as f64, &labels);
        }

        Ok(())
    }
}
