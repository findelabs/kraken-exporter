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
const ASSETS: &str = "https://api.kraken.com/0/public/Assets";
const TICKER: &str = "https://api.kraken.com/0/public/Ticker";
const REFERENCE_CURRENCIES: &'static [&'static str] = &["AUD", "CAD", "BTC", "ETH", "EUR", "GBP", "JPY", "USD", "XBT", "USDT", "USDC"];

#[derive(Clone, Debug)]
pub struct State {
    pub client: HttpsClient,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assets {
  error: Vec<String>,
  result: HashMap<String, Asset>
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
  aclass: String,
  altname: String,
  decimals: u32,
  display_decimals: u32 
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetPairs {
  error: Vec<String>,
  result: HashMap<String, AssetPair>
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetPair {
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

        let req = Request::builder()
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

    pub async fn generate(&self) -> Result<(), RestError> {
        let bytes = self.get(ASSET_PAIRS).await?;
        let asset_pairs: AssetPairs = serde_json::from_slice(&bytes)?;
        log::debug!("{:?}", asset_pairs);

        let bytes = self.get(ASSETS).await?;
        let assets: Assets = serde_json::from_slice(&bytes)?;
        log::debug!("{:?}", assets);

        let mut vec: Vec<String> = Vec::new();
        for (_, asset) in assets.result.iter() {
            log::debug!("Looping over {}", asset.altname);
            for reference_currency in REFERENCE_CURRENCIES {
                let pair = format!("{}{}", asset.altname, reference_currency);
                log::trace!("Checking if {} exists", pair);
                if asset_pairs.result.contains_key(&pair) {
                    log::debug!("{} pair exists", pair);
                    vec.push(pair);
                }

                let pair = format!("X{}X{}", asset.altname, reference_currency);
                log::trace!("Checking if {} exists", pair);
                if asset_pairs.result.contains_key(&pair) {
                    log::debug!("{} pair exists", pair);
                    vec.push(pair);
                }

                let pair = format!("X{}Z{}", asset.altname, reference_currency);
                log::trace!("Checking if {} exists", pair);
                if asset_pairs.result.contains_key(&pair) {
                    log::debug!("{} pair exists", pair);
                    vec.push(pair);
                }
            }
        }

        let assets_query = vec.join(",");
        log::debug!("{:#?}", assets_query);

        let url = format!("{}?pair={}", TICKER, assets_query);
        log::debug!("url: {}", url);
        let bytes = self.get(&url).await?;
        let tickers: Tickers = serde_json::from_slice(&bytes)?;

        for (asset, value) in tickers.result.iter() {
            let asset_pair = &asset_pairs.result.get(asset).unwrap();
//            let wsname = asset_pair.wsname.to_string();
            let wsname_split: Vec<&str> = asset_pair.wsname.split('/').collect();

            let labels = [
                ("currency", wsname_split[0].to_string()),
                ("reference_currency", wsname_split[1].to_string()),
                ("pair", asset_pair.wsname.to_string())
            ];
            gauge!("exchange_rate", value.c[0].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_volume_daily", value.v[1].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_rate_average", value.p[0].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_rate_average_last_day", value.p[1].parse::<f64>().unwrap(), &labels);
            gauge!("exchange_trades_daily", value.t[1] as f64, &labels);
        }

        Ok(())
    }
}
