use hyper::client::conn;
use iced::futures;  
use iced::subscription::{self, Subscription};
use serde::{de, Deserializer};
use futures::sink::SinkExt;

use serde_json::Value;
use crate::{Ticker, Timeframe};

use bytes::Bytes;

use sonic_rs::{LazyValue, JsonValueTrait};
use sonic_rs::{Deserialize, Serialize}; 
use sonic_rs::{to_array_iter, to_object_iter_unchecked};

use anyhow::{Context, Result};

use fastwebsockets::{Frame, FragmentCollector, OpCode};
use http_body_util::Empty;
use hyper::header::{CONNECTION, UPGRADE};
use hyper::upgrade::Upgraded;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, OwnedTrustAnchor};
use tokio_rustls::TlsConnector;

use crate::data_providers::{LocalDepthCache, Trade, Depth, Order, FeedLatency, Kline};

#[allow(clippy::large_enum_variant)]
enum State {
    Disconnected,
    Connected(
        FragmentCollector<TokioIo<Upgraded>>
    ),
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected(Connection),
    Disconnected,
    DepthReceived(Ticker, FeedLatency, i64, Depth, Vec<Trade>),
    KlineReceived(Ticker, Kline, Timeframe),
}

#[derive(Debug, Clone)]
pub struct Connection;

impl<'de> Deserialize<'de> for Order {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let arr: Vec<&str> = Vec::<&str>::deserialize(deserializer)?;
        let price: f32 = arr[0].parse::<f32>().map_err(serde::de::Error::custom)?;
        let qty: f32 = arr[1].parse::<f32>().map_err(serde::de::Error::custom)?;
        Ok(Order { price, qty })
    }
}
#[derive(Debug, Deserialize, Clone)]
pub struct FetchedDepth {
    #[serde(rename = "lastUpdateId")]
    update_id: i64,
    #[serde(rename = "T")]
    time: i64,
    #[serde(rename = "bids")]
    bids: Vec<Order>,
    #[serde(rename = "asks")]
    asks: Vec<Order>,
}

#[derive(Serialize, Deserialize, Debug)]
struct SonicDepth {
	#[serde(rename = "T")]
	time: u64,
	#[serde(rename = "U")]
	first_id: u64,
	#[serde(rename = "u")]
	final_id: u64,
	#[serde(rename = "pu")]
	prev_final_id: u64,
	#[serde(rename = "b")]
	bids: Vec<BidAsk>,
	#[serde(rename = "a")]
	asks: Vec<BidAsk>,
}

#[derive(Serialize, Deserialize, Debug)]
struct BidAsk {
	#[serde(rename = "0")]
	price: String,
	#[serde(rename = "1")]
	qty: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct SonicTrade {
	#[serde(rename = "T")]
	time: u64,
	#[serde(rename = "p")]
	price: String,
	#[serde(rename = "q")]
	qty: String,
	#[serde(rename = "m")]
	is_sell: bool,
}

#[derive(Deserialize, Debug, Clone)]
struct SonicKline {
    #[serde(rename = "t")]
    time: u64,
    #[serde(rename = "o")]
    open: String,
    #[serde(rename = "h")]
    high: String,
    #[serde(rename = "l")]
    low: String,
    #[serde(rename = "c")]
    close: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "V")]
    taker_buy_base_asset_volume: String,
    #[serde(rename = "i")]
    interval: String,
}

#[derive(Deserialize, Debug, Clone)]
struct SonicKlineWrap {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "k")]
    kline: SonicKline,
}

#[derive(Debug)]
enum StreamData {
	Trade(SonicTrade),
	Depth(SonicDepth),
    Kline(Ticker, SonicKline),
}

#[derive(Debug)]
enum StreamName {
    Depth,
    Trade,
    Kline,
    Unknown,
}
impl StreamName {
    fn from_stream_type(stream_type: &str) -> Self {
        if let Some(after_at) = stream_type.split('@').nth(1) {
            match after_at {
                _ if after_at.starts_with("dep") => StreamName::Depth,
                _ if after_at.starts_with("tra") => StreamName::Trade,
                _ if after_at.starts_with("kli") => StreamName::Kline,
                _ => StreamName::Unknown,
            }
        } else {
            StreamName::Unknown
        }
    }
}

#[derive(Debug)]
enum StreamWrapper {
	Trade,
	Depth,
    Kline,
}

fn feed_de(bytes: &Bytes) -> Result<StreamData> {
	let mut stream_type: Option<StreamWrapper> = None;

	let iter: sonic_rs::ObjectJsonIter = unsafe { to_object_iter_unchecked(bytes) };

	for elem in iter {
		let (k, v) = elem
            .context("Error parsing stream")?;

		if k == "stream" {
			if let Some(val) = v.as_str() {
                match StreamName::from_stream_type(val) {
					StreamName::Depth => {
						stream_type = Some(StreamWrapper::Depth);
					},
					StreamName::Trade => {
						stream_type = Some(StreamWrapper::Trade);
					},
                    StreamName::Kline => {
                        stream_type = Some(StreamWrapper::Kline);
                    },
					_ => {
                        eprintln!("Unknown stream name");
                    }
				}
			}
		} else if k == "data" {
			match stream_type {
				Some(StreamWrapper::Trade) => {
					let trade: SonicTrade = sonic_rs::from_str(&v.as_raw_faststr())
						.context("Error parsing trade")?;

					return Ok(StreamData::Trade(trade));
				},
				Some(StreamWrapper::Depth) => {
					let depth: SonicDepth = sonic_rs::from_str(&v.as_raw_faststr())
						.context("Error parsing depth")?;

					return Ok(StreamData::Depth(depth));
				},
                Some(StreamWrapper::Kline) => {
                    let kline_wrap: SonicKlineWrap = sonic_rs::from_str(&v.as_raw_faststr())
                        .context("Error parsing kline")?;

                    let ticker = match &kline_wrap.symbol[..] {
                        "BTCUSDT" => Ticker::BTCUSDT,
                        "ETHUSDT" => Ticker::ETHUSDT,
                        "SOLUSDT" => Ticker::SOLUSDT,
                        "LTCUSDT" => Ticker::LTCUSDT,
                        _ => Ticker::BTCUSDT,
                    };

                    return Ok(StreamData::Kline(ticker, kline_wrap.kline));
                },
				_ => {
					eprintln!("Unknown stream type");
				}
			}
		} else {
			eprintln!("Unknown data: {:?}", k);
		}
	}

	Err(anyhow::anyhow!("Unknown data"))
}

fn tls_connector() -> Result<TlsConnector> {
	let mut root_store = tokio_rustls::rustls::RootCertStore::empty();

	root_store.add_trust_anchors(
		webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
			OwnedTrustAnchor::from_subject_spki_name_constraints(
			ta.subject,
			ta.spki,
			ta.name_constraints,
			)
		}),
	);

	let config = ClientConfig::builder()
		.with_safe_defaults()
		.with_root_certificates(root_store)
		.with_no_client_auth();

	Ok(TlsConnector::from(std::sync::Arc::new(config)))
}

async fn connect(domain: &str, streams: &str) -> Result<FragmentCollector<TokioIo<Upgraded>>> {
	let mut addr = String::from(domain);
	addr.push_str(":443");

	let tcp_stream: TcpStream = TcpStream::connect(&addr).await?;
	let tls_connector: TlsConnector = tls_connector().unwrap();
	let domain: tokio_rustls::rustls::ServerName =
	tokio_rustls::rustls::ServerName::try_from(domain).map_err(|_| {
		std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid dnsname")
	})?;

	let tls_stream: tokio_rustls::client::TlsStream<TcpStream> = tls_connector.connect(domain, tcp_stream).await?;

    let url = format!("wss://{}/stream?streams={}", &addr, streams);
    println!("Connecting to {}", url);

	let req: Request<Empty<Bytes>> = Request::builder()
	.method("GET")
	.uri(url)
	.header("Host", &addr)
	.header(UPGRADE, "websocket")
	.header(CONNECTION, "upgrade")
	.header(
		"Sec-WebSocket-Key",
		fastwebsockets::handshake::generate_key(),
	)
	.header("Sec-WebSocket-Version", "13")
	.body(Empty::<Bytes>::new())?;

	let (ws, _) = fastwebsockets::handshake::client(&SpawnExecutor, req, tls_stream).await?;
	Ok(FragmentCollector::new(ws))
}
struct SpawnExecutor;

impl<Fut> hyper::rt::Executor<Fut> for SpawnExecutor
where
  Fut: std::future::Future + Send + 'static,
  Fut::Output: Send + 'static,
{
  fn execute(&self, fut: Fut) {
	tokio::task::spawn(fut);
  }
}

pub fn connect_market_stream(stream: Ticker) -> Subscription<Event> {
    subscription::channel(
        stream,
        100,
        move |mut output| async move {
            let mut state = State::Disconnected;     
            let mut trades_buffer: Vec<Trade> = Vec::new(); 

            let selected_ticker = stream;

            let symbol_str = match selected_ticker {
                Ticker::BTCUSDT => "btcusdt",
                Ticker::ETHUSDT => "ethusdt",
                Ticker::SOLUSDT => "solusdt",
                Ticker::LTCUSDT => "ltcusdt",
            };

            let stream_1 = format!("{symbol_str}@trade");
            let stream_2 = format!("{symbol_str}@depth@100ms");

            let mut orderbook: LocalDepthCache = LocalDepthCache::new();

            let mut already_fetching: bool = false;

            let mut prev_id: u64 = 0;

            let mut trade_latencies: Vec<i64> = Vec::new();

            loop {
                match &mut state {
                    State::Disconnected => {        
                        let streams = format!("{stream_1}/{stream_2}");

                        let domain: &str = "fstream.binance.com";

                        if let Ok(websocket) = connect(domain, streams.as_str()
                        )
                        .await {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                                                
                            tokio::spawn(async move {
                                let fetched_depth = fetch_depth(selected_ticker).await;

                                let depth: LocalDepthCache = match fetched_depth {
                                    Ok(depth) => {
                                        LocalDepthCache {
                                            last_update_id: depth.update_id,
                                            time: depth.time,
                                            bids: depth.bids,
                                            asks: depth.asks,
                                        }
                                    },
                                    Err(_) => return,
                                };

                                let _ = tx.send(depth);
                            });
                            match rx.await {
                                Ok(depth) => {
                                    orderbook.fetched(depth);
                                    state = State::Connected(websocket);
                                },
                                Err(_) => output.send(Event::Disconnected).await.expect("Trying to send disconnect event..."),
                            }
                            
                        } else {
                            tokio::time::sleep(tokio::time::Duration::from_secs(1))
                           .await;
                           let _ = output.send(Event::Disconnected).await;
                        }
                    },
                    State::Connected(ws) => {
                        let feed_latency: FeedLatency;

                        match ws.read_frame().await {
                            Ok(msg) => match msg.opcode {
                                OpCode::Text => {                    
                                    let json_bytes: Bytes = Bytes::from(msg.payload.to_vec());
                    
                                    if let Ok(data) = feed_de(&json_bytes) {
                                        match data {
                                            StreamData::Trade(de_trade) => {
                                                let trade = Trade {
                                                    time: de_trade.time as i64,
                                                    is_sell: de_trade.is_sell,
                                                    price: str_f32_parse(&de_trade.price),
                                                    qty: str_f32_parse(&de_trade.qty),
                                                };

                                                trade_latencies.push(
                                                    chrono::Utc::now().timestamp_millis() - trade.time
                                                );

                                                trades_buffer.push(trade);
                                            },
                                            StreamData::Depth(de_depth) => {
                                                if already_fetching {
                                                    println!("Already fetching...\n");
    
                                                    continue;
                                                }
    
                                                let last_update_id = orderbook.get_fetch_id() as u64;
                                                
                                                if (de_depth.final_id <= last_update_id) || last_update_id == 0 {
                                                    continue;
                                                }
    
                                                if prev_id == 0 && (de_depth.first_id > last_update_id + 1) || (last_update_id + 1 > de_depth.final_id) {
                                                    println!("Out of sync at first event. Trying to resync...\n");
    
                                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                                    already_fetching = true;
    
                                                    tokio::spawn(async move {
                                                        let fetched_depth = fetch_depth(selected_ticker).await;
    
                                                        let depth: LocalDepthCache = match fetched_depth {
                                                            Ok(depth) => {
                                                                LocalDepthCache {
                                                                    last_update_id: depth.update_id,
                                                                    time: depth.time,
                                                                    bids: depth.bids,
                                                                    asks: depth.asks,
                                                                }
                                                            },
                                                            Err(_) => return,
                                                        };
    
                                                        let _ = tx.send(depth);
                                                    });
                                                    match rx.await {
                                                        Ok(depth) => {
                                                            orderbook.fetched(depth)
                                                        },
                                                        Err(_) => {
                                                            state = State::Disconnected;
                                                            output.send(Event::Disconnected).await.expect("Trying to send disconnect event...");
                                                        },
                                                    }
                                                    already_fetching = false;
                                                }
                                        
                                                if (prev_id == 0) || (prev_id == de_depth.prev_final_id) {
                                                    let time = de_depth.time as i64;
    
                                                    let depth_latency = chrono::Utc::now().timestamp_millis() - time;
    
                                                    let depth_update = LocalDepthCache {
                                                        last_update_id: de_depth.final_id as i64,
                                                        time,
                                                        bids: de_depth.bids.iter().map(|x| Order { price: str_f32_parse(&x.price), qty: str_f32_parse(&x.qty) }).collect(),
                                                        asks: de_depth.asks.iter().map(|x| Order { price: str_f32_parse(&x.price), qty: str_f32_parse(&x.qty) }).collect(),
                                                    };
    
                                                    let (local_bids, local_asks) = orderbook.update_levels(depth_update);
    
                                                    let current_depth = Depth {
                                                        time,
                                                        bids: local_bids,
                                                        asks: local_asks,
                                                    };
                                                    
                                                    let avg_trade_latency = if !trade_latencies.is_empty() {
                                                        let avg = trade_latencies.iter().sum::<i64>() / trade_latencies.len() as i64;
                                                        trade_latencies.clear();
                                                        Some(avg)
                                                    } else {
                                                        None
                                                    };
                                                    feed_latency = FeedLatency {
                                                        time,
                                                        depth_latency,
                                                        trade_latency: avg_trade_latency,
                                                    };
    
                                                    let _ = output.send(
                                                        Event::DepthReceived(
                                                            selected_ticker,
                                                            feed_latency,
                                                            time, 
                                                            current_depth,
                                                            std::mem::take(&mut trades_buffer)
                                                        )
                                                    ).await;
    
                                                    prev_id = de_depth.final_id;
                                                } else {
                                                    eprintln!("Out of sync...\n");
                                                }
                                            },
                                            _ => {}
                                        }
                                    } else {
                                        eprintln!("\nUnknown data: {:?}", &json_bytes);
                                    }
                                }
                                OpCode::Close => {
                                    eprintln!("Connection closed");
                                    let _ = output.send(Event::Disconnected).await;
                                }
                                _ => {}
                            },
                            Err(e) => {
                                println!("Error reading frame: {}", e);
                            }
                        };
                    }
                }
            }
        },
    )
}

pub fn connect_kline_stream(vec: Vec<(Ticker, Timeframe)>) -> Subscription<Event> {
    struct Connect;

    subscription::channel(
        std::any::TypeId::of::<Connect>(),
        100,
        move |mut output| async move {
            let mut state = State::Disconnected;    

            let mut symbol_str: &str = "";

            let stream_str = vec.iter().map(|(ticker, timeframe)| {
                symbol_str = match ticker {
                    Ticker::BTCUSDT => "btcusdt",
                    Ticker::ETHUSDT => "ethusdt",
                    Ticker::SOLUSDT => "solusdt",
                    Ticker::LTCUSDT => "ltcusdt",
                };
                let timeframe_str = match timeframe {
                    Timeframe::M1 => "1m",
                    Timeframe::M3 => "3m",
                    Timeframe::M5 => "5m",
                    Timeframe::M15 => "15m",
                    Timeframe::M30 => "30m",
                };
                format!("{symbol_str}@kline_{timeframe_str}")
            }).collect::<Vec<String>>().join("/");

            loop {
                match &mut state {
                    State::Disconnected => {
                        let domain: &str = "fstream.binance.com";

                        let streams = stream_str.as_str();
                        
                        if let Ok(websocket) = connect(
                            domain, streams
                        )
                        .await {
                           state = State::Connected(websocket);
                        } else {
                            tokio::time::sleep(tokio::time::Duration::from_secs(1))
                           .await;
                           let _ = output.send(Event::Disconnected).await;
                        }
                    },
                    State::Connected(ws) => {
                        match ws.read_frame().await {
                            Ok(msg) => match msg.opcode {
                                OpCode::Text => {                    
                                    let json_bytes: Bytes = Bytes::from(msg.payload.to_vec());
                    
                                    if let Ok(StreamData::Kline(ticker, de_kline)) = feed_de(&json_bytes) {
                                        let kline = Kline {
                                            time: de_kline.time,
                                            open: str_f32_parse(&de_kline.open),
                                            high: str_f32_parse(&de_kline.high),
                                            low: str_f32_parse(&de_kline.low),
                                            close: str_f32_parse(&de_kline.close),
                                            volume: str_f32_parse(&de_kline.volume),
                                            buy_volume: str_f32_parse(&de_kline.taker_buy_base_asset_volume),
                                        };

                                        if let Some(timeframe) = vec.iter().find(|(_, tf)| tf.to_string() == de_kline.interval) {
                                            let _ = output.send(Event::KlineReceived(ticker, kline, timeframe.1)).await;
                                        }
                                    } else {
                                        eprintln!("\nUnknown data: {:?}", &json_bytes);
                                    }
                                }
                                _ => {}
                            }, 
                            Err(e) => {
                                eprintln!("Error reading frame: {}", e);
                            }
                        }
                    }
                }
            }
        },
    )
}

fn str_f32_parse(s: &str) -> f32 {
    s.parse::<f32>().unwrap_or_else(|e| {
        eprintln!("Failed to parse float: {}, error: {}", s, e);
        0.0
    })
}

mod string_to_f32 {
    use serde::{self, Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f32, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = <&str>::deserialize(deserializer)?;
        s.parse::<f32>().map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Debug, Clone)]
struct FetchedKlines (
    u64,
    #[serde(with = "string_to_f32")] f32,
    #[serde(with = "string_to_f32")] f32,
    #[serde(with = "string_to_f32")] f32,
    #[serde(with = "string_to_f32")] f32,
    #[serde(with = "string_to_f32")] f32,
    u64,
    String,
    u32,
    #[serde(with = "string_to_f32")] f32,
    String,
    String,
);
impl From<FetchedKlines> for Kline {
    fn from(fetched: FetchedKlines) -> Self {
        Self {
            time: fetched.0,
            open: fetched.1,
            high: fetched.2,
            low: fetched.3,
            close: fetched.4,
            volume: fetched.5,
            buy_volume: fetched.9,
        }
    }
}

pub async fn fetch_klines(ticker: Ticker, timeframe: Timeframe) -> Result<Vec<Kline>, reqwest::Error> {
    let symbol_str = match ticker {
        Ticker::BTCUSDT => "btcusdt",
        Ticker::ETHUSDT => "ethusdt",
        Ticker::SOLUSDT => "solusdt",
        Ticker::LTCUSDT => "ltcusdt",
    };
    let timeframe_str = match timeframe {
        Timeframe::M1 => "1m",
        Timeframe::M3 => "3m",
        Timeframe::M5 => "5m",
        Timeframe::M15 => "15m",
        Timeframe::M30 => "30m",
    };

    let url = format!("https://fapi.binance.com/fapi/v1/klines?symbol={symbol_str}&interval={timeframe_str}&limit=720");

    let response = reqwest::get(&url).await?;
    let text = response.text().await?;
    let fetched_klines: Result<Vec<FetchedKlines>, _> = serde_json::from_str(&text);
    let klines: Vec<Kline> = fetched_klines.unwrap().into_iter().map(Kline::from).collect();

    Ok(klines)
}

pub async fn fetch_depth(ticker: Ticker) -> Result<FetchedDepth, reqwest::Error> {
    let symbol_str = match ticker {
        Ticker::BTCUSDT => "btcusdt",
        Ticker::ETHUSDT => "ethusdt",
        Ticker::SOLUSDT => "solusdt",
        Ticker::LTCUSDT => "ltcusdt",
    };

    let url = format!("https://fapi.binance.com/fapi/v1/depth?symbol={symbol_str}&limit=500");

    let response = reqwest::get(&url).await?;
    let text = response.text().await?;
    let depth: FetchedDepth = serde_json::from_str(&text).unwrap();

    Ok(depth)
}

pub async fn fetch_ticksize(ticker: Ticker) -> Result<f32, reqwest::Error> {
    let symbol_str = match ticker {
        Ticker::BTCUSDT => "BTCUSDT",
        Ticker::ETHUSDT => "ETHUSDT",
        Ticker::SOLUSDT => "SOLUSDT",
        Ticker::LTCUSDT => "LTCUSDT",
    };

    let url = format!("https://fapi.binance.com/fapi/v1/exchangeInfo");

    let response = reqwest::get(&url).await?;
    let text = response.text().await?;
    let exchange_info: Value = serde_json::from_str(&text).unwrap();

    let symbols = exchange_info["symbols"].as_array().unwrap();

    let symbol = symbols.iter().find(|x| x["symbol"].as_str().unwrap() == symbol_str).unwrap();

    let tick_size = symbol["filters"].as_array().unwrap().iter().find(|x| x["filterType"].as_str().unwrap() == "PRICE_FILTER").unwrap()["tickSize"].as_str().unwrap().parse::<f32>().unwrap();

    Ok(tick_size)
}

pub async fn fetch_server_time() -> Result<i64> {
    let url = "https://fapi.binance.com/fapi/v1/time";

    let response = reqwest::get(url).await.context("Failed to send request")?;
    let text = response.text().await.context("Failed to read response")?;
    
    let server_time: Value = serde_json::from_str(&text).context("Failed to parse JSON")?;

    if let Some(time) = server_time["serverTime"].as_i64() {
        Ok(time)
    } else {
        anyhow::bail!("Invalid server time")
    }
}