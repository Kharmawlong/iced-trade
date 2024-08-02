pub mod pane;

use pane::SerializablePane;
pub use pane::{Uuid, PaneState, PaneContent, PaneSettings};
use serde::{Deserialize, Serialize};
use serde_json::to_string;

use crate::{
    charts::{candlestick::CandlestickChart, footprint::FootprintChart, Message}, 
    data_providers::{
        Depth, Exchange, Kline, TickMultiplier, Ticker, Timeframe, Trade
    }, 
    StreamType
};

use std::{collections::{HashMap, HashSet}, io::Read, rc::Rc};
use iced::widget::pane_grid::{self, Configuration};

pub struct Dashboard {
    pub panes: pane_grid::State<PaneState>,
    pub focus: Option<pane_grid::Pane>,
    pub pane_lock: bool,
    pub show_layout_modal: bool,
}
impl Dashboard {
    pub fn empty() -> Self {
        let pane_config: Configuration<PaneState> = Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: 0.8,
            a: Box::new(Configuration::Split {
                axis: pane_grid::Axis::Horizontal,
                ratio: 0.4,
                a: Box::new(Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(
                        PaneState { 
                            id: Uuid::new_v4(), 
                            show_modal: false, 
                            stream: vec![],
                            content: PaneContent::Starter,
                            settings: PaneSettings::default(),
                        })
                    ),
                    b: Box::new(Configuration::Pane(
                        PaneState { 
                            id: Uuid::new_v4(), 
                            show_modal: false, 
                            stream: vec![],
                            content: PaneContent::Starter,
                            settings: PaneSettings::default(),
                        })
                    ),
                }),
                b: Box::new(Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(
                        PaneState { 
                            id: Uuid::new_v4(), 
                            show_modal: false, 
                            stream: vec![],
                            content: PaneContent::Starter,
                            settings: PaneSettings::default(),
                        })                      
                    ),
                    b: Box::new(Configuration::Pane(
                        PaneState { 
                            id: Uuid::new_v4(), 
                            show_modal: false, 
                            stream: vec![],
                            content: PaneContent::Starter,
                            settings: PaneSettings::default(),
                        })
                    ),
                }),
            }),
            b: Box::new(Configuration::Pane(
                PaneState { 
                    id: Uuid::new_v4(), 
                    show_modal: false, 
                    stream: vec![],
                    content: PaneContent::Starter,
                    settings: PaneSettings::default(),
                })
            ),
        };
        
        Self { 
            panes: pane_grid::State::with_configuration(pane_config),
            focus: None,
            pane_lock: false,
            show_layout_modal: false,
        }
    }

    pub fn from_config(panes: Configuration<PaneState>) -> Self {
        Self {
            panes: pane_grid::State::with_configuration(panes),
            focus: None,
            pane_lock: false,
            show_layout_modal: false,
        }
    }

    pub fn replace_new_pane(&mut self, pane: pane_grid::Pane) {
        if let Some(pane) = self.panes.get_mut(pane) {
            *pane = PaneState::new(Uuid::new_v4(), vec![], PaneSettings::default());
        }
    }

    pub fn update_chart_state(&mut self, pane_id: Uuid, message: Message) -> Result<(), &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                match pane_state.content {
                    PaneContent::Heatmap(ref mut chart) => {
                        chart.update(&message);

                        return Ok(());
                    },
                    PaneContent::Footprint(ref mut chart) => {
                        chart.update(&message);

                        return Ok(());
                    },
                    PaneContent::Candlestick(ref mut chart) => {
                        chart.update(&message);

                        return Ok(());
                    },
                    _ => {
                        return Err("No chart found");
                    }
                }
            }
        }
        Err("No pane found")
    }

    pub fn get_pane_stream_mut(&mut self, pane_id: Uuid) -> Result<&mut Vec<StreamType>, &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                return Ok(&mut pane_state.stream);
            }
        }
        Err("No pane found")
    }

    pub fn get_pane_settings_mut(&mut self, pane_id: Uuid) -> Result<&mut PaneSettings, &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                return Ok(&mut pane_state.settings);
            }
        }
        Err("No pane found")
    }

    pub fn set_pane_content(&mut self, pane_id: Uuid, content: PaneContent) -> Result<(), &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                pane_state.content = content;

                return Ok(());
            }
        }
        Err("No pane found")
    }

    pub fn pane_change_ticksize(&mut self, pane_id: Uuid, new_tick_multiply: TickMultiplier) -> Result<(), &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                pane_state.settings.tick_multiply = Some(new_tick_multiply);

                if let Some(min_tick_size) = pane_state.settings.min_tick_size {
                    match pane_state.content {
                        PaneContent::Footprint(ref mut chart) => {
                            chart.change_tick_size(
                                new_tick_multiply.multiply_with_min_tick_size(min_tick_size)
                            );
                            
                            return Ok(());
                        },
                        _ => {
                            return Err("No footprint chart found");
                        }
                    }
                } else {
                    return Err("No min tick size found");
                }
            }
        }
        Err("No pane found")
    }
    
    pub fn pane_change_timeframe(&mut self, pane_id: Uuid, new_timeframe: Timeframe) -> Result<&StreamType, &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                pane_state.settings.selected_timeframe = Some(new_timeframe);

                for stream_type in pane_state.stream.iter_mut() {
                    match stream_type {
                        StreamType::Kline { timeframe, .. } => {
                            *timeframe = new_timeframe;

                            match pane_state.content {
                                PaneContent::Candlestick(_) => {
                                    return Ok(stream_type);
                                },
                                PaneContent::Footprint(_) => {
                                    return Ok(stream_type);
                                },
                                _ => {}
                            }
                        },
                        _ => {}
                    }
                }
            }
        }
        Err("No pane found")
    }

    pub fn pane_set_size_filter(&mut self, pane_id: Uuid, new_size_filter: f32) -> Result<(), &str> {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                match pane_state.content {
                    PaneContent::Heatmap(ref mut chart) => {
                        chart.set_size_filter(new_size_filter);
                        
                        return Ok(());
                    },
                    PaneContent::TimeAndSales(ref mut chart) => {
                        chart.set_size_filter(new_size_filter);
                        
                        return Ok(());
                    },
                    _ => {
                        return Err("No footprint chart found");
                    }
                }
            }
        }
        Err("No pane found")
    }

    pub fn insert_klines_vec(&mut self, stream_type: &StreamType, klines: &Vec<Kline>, pane_id: Uuid) {
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.id == pane_id {
                match stream_type {
                    StreamType::Kline { timeframe, .. } => {
                        let timeframe_u16 = timeframe.to_minutes();

                        match &mut pane_state.content {
                            PaneContent::Candlestick(chart) => {
                                *chart = CandlestickChart::new(klines.to_vec(), timeframe_u16);
                            },
                            PaneContent::Footprint(chart) => {
                                let raw_trades = chart.get_raw_trades();

                                let tick_size = chart.get_tick_size();

                                *chart = FootprintChart::new(timeframe_u16, tick_size, klines.to_vec(), raw_trades);
                            },
                            _ => {}
                        }
                    },
                    _ => {}
                }
            }
        }
    }

    pub fn update_latest_klines(&mut self, stream_type: &StreamType, kline: &Kline) -> Result<(), &str> {
        let mut found_match = false;
    
        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.matches_stream(&stream_type) {
                match &mut pane_state.content {
                    PaneContent::Candlestick(chart) => chart.update_latest_kline(kline),
                    PaneContent::Footprint(chart) => chart.update_latest_kline(kline),
                    _ => {}
                }
                found_match = true;
            }
        }
    
        if found_match {
            Ok(())
        } else {
            Err("No matching pane found for the stream")
        }
    }

    pub fn update_depth_and_trades(&mut self, stream_type: StreamType, depth_update_t: i64, depth: Depth, trades_buffer: Vec<Trade>) -> Result<(), &str> {
        let mut found_match = false;
        
        let depth = Rc::new(depth);

        let trades_buffer = trades_buffer.into_boxed_slice();

        for (_, pane_state) in self.panes.iter_mut() {
            if pane_state.matches_stream(&stream_type) {
                match &mut pane_state.content {
                    PaneContent::Heatmap(chart) => {
                        chart.insert_datapoint(&trades_buffer, depth_update_t, Rc::clone(&depth));
                    },
                    PaneContent::Footprint(chart) => {
                        chart.insert_datapoint(&trades_buffer, depth_update_t);
                    },
                    PaneContent::TimeAndSales(chart) => {
                        chart.update(&trades_buffer);
                    },
                    _ => {}
                }

                found_match = true;
            }
        }

        if found_match {
            Ok(())
        } else {
            Err("No matching pane found for the stream")
        }
    }

    pub fn get_all_diff_streams(&self) -> HashMap<Exchange, HashMap<Ticker, HashSet<StreamType>>> {
        let mut pane_streams = HashMap::new();

        for (_, pane_state) in self.panes.iter() {
            for stream_type in &pane_state.stream {
                match stream_type {
                    StreamType::Kline { exchange, ticker, timeframe } => {
                        let exchange = exchange.clone();
                        let ticker = ticker.clone();
                        let timeframe = timeframe.clone();

                        let exchange_map = pane_streams.entry(exchange.clone()).or_insert(HashMap::new());
                        let ticker_map = exchange_map.entry(ticker).or_insert(HashSet::new());
                        ticker_map.insert(StreamType::Kline { exchange, ticker, timeframe });
                    },
                    StreamType::DepthAndTrades { exchange, ticker } => {
                        let exchange = exchange.clone();
                        let ticker = ticker.clone();

                        let exchange_map = pane_streams.entry(exchange).or_insert(HashMap::new());
                        let ticker_map = exchange_map.entry(ticker).or_insert(HashSet::new());
                        ticker_map.insert(StreamType::DepthAndTrades { exchange, ticker });
                    },
                    _ => {}
                }
            }
        }

        pane_streams
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SerializableDashboard {
    pub pane: SerializablePane,
}

impl<'a> From<&'a Dashboard> for SerializableDashboard {
    fn from(dashboard: &'a Dashboard) -> Self {
        use pane_grid::Node;

        fn from_layout(panes: &pane_grid::State<PaneState>, node: pane_grid::Node) -> SerializablePane {
            match node {
                Node::Split {
                    axis, ratio, a, b, ..
                } => SerializablePane::Split {
                    axis: match axis {
                        pane_grid::Axis::Horizontal => pane::Axis::Horizontal,
                        pane_grid::Axis::Vertical => pane::Axis::Vertical,
                    },
                    ratio,
                    a: Box::new(from_layout(panes, *a)),
                    b: Box::new(from_layout(panes, *b)),
                },
                Node::Pane(pane) => panes
                    .get(pane)
                    .map(SerializablePane::from)
                    .unwrap_or(SerializablePane::Starter),
            }
        }

        let layout = dashboard.panes.layout().clone();

        SerializableDashboard {
            pane: from_layout(&dashboard.panes, layout),
        }
    }
}

use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_json_to_file(json: &str, file_path: &str) -> std::io::Result<()> {
    let path = Path::new(file_path);
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn read_dashboard_from_file(file_path: &str) -> Result<SerializableDashboard, Box<dyn std::error::Error>> {
    let path = Path::new(file_path);
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let dashboard: SerializableDashboard = serde_json::from_str(&contents)?;
    Ok(dashboard)
}