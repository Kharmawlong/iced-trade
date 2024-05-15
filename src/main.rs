#![windows_subsystem = "windows"]

mod data_providers;
use data_providers::binance::{user_data, market_data};
mod charts;
use charts::{candlesticks, custom_line::{self, CustomLine}, heatmap};

use crate::heatmap::LineChart;
use crate::candlesticks::CandlestickChart;

use std::time::Instant;
use std::cell::RefCell;
use chrono::{NaiveDateTime, DateTime, Utc};
use iced::{
    alignment, executor, font, theme::{self, Custom}, widget::{
        button, canvas, checkbox, pick_list, text_input, tooltip, Column, Container, Row, Slider, Space, Text
    }, Alignment, Application, Color, Command, Element, Font, Length, Renderer, Settings, Size, Subscription, Theme, Vector
};

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{
    container, row, scrollable, text, responsive
};
use iced_table::table;
use futures::TryFutureExt;
use plotters_iced::sample::lttb::DataPoint;

use iced_aw::menu::{Item, Menu};
use iced_aw::{menu_bar, menu_items, modal, Card};

use std::collections::HashMap;

struct Wrapper<'a>(&'a DateTime<Utc>, &'a f32);
impl DataPoint for Wrapper<'_> {
    #[inline]
    fn x(&self) -> f64 {
        self.0.timestamp() as f64
    }
    #[inline]
    fn y(&self) -> f64 {
        *self.1 as f64
    }
}
impl std::fmt::Display for Ticker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Ticker::BTCUSDT => "BTCUSDT",
                Ticker::ETHUSDT => "ETHUSDT",
                Ticker::SOLUSDT => "SOLUSDT",
                Ticker::LTCUSDT => "LTCUSDT",
            }
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ticker {
    BTCUSDT,
    ETHUSDT,
    SOLUSDT,
    LTCUSDT,
}
impl Ticker {
    const ALL: [Ticker; 4] = [Ticker::BTCUSDT, Ticker::ETHUSDT, Ticker::SOLUSDT, Ticker::LTCUSDT];
}

impl std::fmt::Display for Timeframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Timeframe::M1 => "1m",
                Timeframe::M3 => "3m",
                Timeframe::M5 => "5m",
                Timeframe::M15 => "15m",
                Timeframe::M30 => "30m",
            }
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeframe {
    M1,
    M3,
    M5,
    M15,
    M30,
}
impl Timeframe {
    const ALL: [Timeframe; 5] = [Timeframe::M1, Timeframe::M3, Timeframe::M5, Timeframe::M15, Timeframe::M30];
}

// binance testnet api keys
const API_KEY: &str = "";
const SECRET_KEY: &str = "";

const ICON_BYTES: &[u8] = include_bytes!("fonts/icons.ttf");
const ICON: Font = Font::with_name("icons");

enum Icon {
    Locked,
    Unlocked,
    ResizeFull,
    ResizeSmall,
    Close,
    Layout,
    Cog,
}

impl From<Icon> for char {
    fn from(icon: Icon) -> Self {
        match icon {
            Icon::Unlocked => '\u{E800}',
            Icon::Locked => '\u{E801}',
            Icon::ResizeFull => '\u{E802}',
            Icon::ResizeSmall => '\u{E803}',
            Icon::Close => '\u{E804}',
            Icon::Layout => '\u{E805}',
            Icon::Cog => '\u{E806}',
        }
    }
}

enum WsState {
    Connected(market_data::Connection),
    Disconnected,
}
impl Default for WsState {
    fn default() -> Self {
        Self::Disconnected
    }
}

enum UserWsState {
    Connected(user_data::Connection),
    Disconnected,
}
impl Default for UserWsState {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Debug, Clone, Copy)]
#[derive(Eq, Hash, PartialEq)]
pub enum PaneId {
    HeatmapChart,
    CandlestickChart,
    TimeAndSales,
    TradePanel,
}

#[derive(Debug, Clone, Copy)]
struct Pane {
    id: PaneId,
    show_modal: bool,
}

impl Pane {
    fn new(id: PaneId) -> Self {
        Self { 
            id,
            show_modal: false,
        }
    }
}

fn main() {
    State::run(Settings {
        antialiasing: true,
        ..Settings::default()
    })
    .unwrap();
}

#[derive(Debug, Clone)]
pub enum Message {
    Debug(String),

    CustomLine(custom_line::Message),

    // Market&User data stream
    UserKeySucceed(String),
    UserKeyError,
    UserWsEvent(user_data::Event),
    TickerSelected(Ticker),
    TimeframeSelected(Timeframe),
    ExchangeSelected(&'static str),
    MarketWsEvent(market_data::Event),
    WsToggle(),
    FetchEvent(Result<Vec<market_data::Kline>, std::string::String>),
    UpdateAccInfo(user_data::FetchedBalance),
    
    // Pane grid
    Split(pane_grid::Axis, pane_grid::Pane, PaneId),
    Clicked(pane_grid::Pane),
    Dragged(pane_grid::DragEvent),
    Resized(pane_grid::ResizeEvent),
    Maximize(pane_grid::Pane),
    Restore,
    Close(pane_grid::Pane),
    ToggleLayoutLock,

    // Trading order form
    LimitOrder(String),
    MarketOrder(String),
    CancelOrder(String),
    InputChanged((String, String)),
    OrderCreated(user_data::NewOrder),
    MarketOrderCreated(user_data::NewOrder),
    OrdersFetched(Vec<user_data::NewOrder>),
    OrderFailed(String),

    // Trading table
    TabSelected(usize, String),
    SyncHeader(scrollable::AbsoluteOffset),
    TableResizing(usize, f32),
    TableResized,
    FooterEnabled(bool),
    MinWidthEnabled(bool),

    // Font
    FontLoaded(Result<(), font::Error>),

    // Modal
    OpenModal(pane_grid::Pane),
    CloseModal,

    // Slider
    SliderChanged(PaneId, f32),
    SyncWithHeatmap(bool),
}

struct State {
    trades_chart: Option<heatmap::LineChart>,
    candlestick_chart: Option<candlesticks::CandlestickChart>,
    time_and_sales: Option<TimeAndSales>,
    custom_line: CustomLine,

    // data streams
    listen_key: Option<String>,
    selected_ticker: Option<Ticker>,
    selected_timeframe: Option<Timeframe>,
    selected_exchange: Option<&'static str>,
    ws_state: WsState,
    user_ws_state: UserWsState,
    ws_running: bool,

    // pane grid
    panes_open: HashMap<PaneId, bool>,
    panes: pane_grid::State<Pane>,
    focus: Option<pane_grid::Pane>,
    first_pane: pane_grid::Pane,
    pane_lock: bool,

    // order form
    qty_input_val: RefCell<Option<String>>,
    price_input_val: RefCell<Option<String>>,

    // table
    order_form_active_tab: usize,
    table_active_tab: usize,
    open_orders: Vec<user_data::NewOrder>,
    orders_header: scrollable::Id,
    orders_body: scrollable::Id,
    orders_footer: scrollable::Id,
    orders_columns: Vec<TableColumn>,
    orders_rows: Vec<TableRow>,
    pos_header: scrollable::Id,
    pos_body: scrollable::Id,
    pos_footer: scrollable::Id,
    position_columns: Vec<PosTableColumn>,
    position_rows: Vec<PosTableRow>,
    resize_columns_enabled: bool,
    footer_enabled: bool,
    min_width_enabled: bool,
    account_info_usdt: Option<user_data::FetchedBalance>,

    size_filter_timesales: f32,
    size_filter_heatmap: f32,
    sync_heatmap: bool,
}

impl Application for State {
    type Message = self::Message;
    type Executor = executor::Default;
    type Flags = ();
    type Theme = Theme;

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let (panes, first_pane) = pane_grid::State::new(Pane::new(PaneId::CandlestickChart));

        let mut panes_open = HashMap::new();
        panes_open.insert(PaneId::HeatmapChart, true);
        panes_open.insert(PaneId::CandlestickChart, true);
        panes_open.insert(PaneId::TimeAndSales, false);
        panes_open.insert(PaneId::TradePanel, false);
        (
            Self { 
                size_filter_timesales: 0.0,
                size_filter_heatmap: 0.0,
                sync_heatmap: false,

                trades_chart: None,
                candlestick_chart: None,
                time_and_sales: None,
                custom_line: CustomLine::default(),
                listen_key: None,
                selected_ticker: None,
                selected_timeframe: Some(Timeframe::M1),
                selected_exchange: Some("Binance Futures"),
                ws_state: WsState::Disconnected,
                user_ws_state: UserWsState::Disconnected,
                ws_running: false,
                panes,
                panes_open,
                focus: None,
                first_pane,
                pane_lock: false,
                qty_input_val: RefCell::new(None),
                price_input_val: RefCell::new(None),
                order_form_active_tab: 0,
                table_active_tab: 0,
                open_orders: vec![],
                orders_header: scrollable::Id::unique(),
                orders_body: scrollable::Id::unique(),
                orders_footer: scrollable::Id::unique(),
                pos_header: scrollable::Id::unique(),
                pos_body: scrollable::Id::unique(),
                pos_footer: scrollable::Id::unique(),
                resize_columns_enabled: true,
                footer_enabled: true,
                min_width_enabled: true,
                orders_columns: vec![
                    TableColumn::new(ColumnKind::UpdateTime),
                    TableColumn::new(ColumnKind::Symbol),
                    TableColumn::new(ColumnKind::OrderType),
                    TableColumn::new(ColumnKind::Side),
                    TableColumn::new(ColumnKind::Price),
                    TableColumn::new(ColumnKind::OrigQty),
                    TableColumn::new(ColumnKind::ExecutedQty),
                    TableColumn::new(ColumnKind::ReduceOnly),
                    TableColumn::new(ColumnKind::TimeInForce),
                    TableColumn::new(ColumnKind::CancelOrder),
                ],
                orders_rows: vec![],
                position_columns: vec![
                    PosTableColumn::new(PosColumnKind::Symbol),
                    PosTableColumn::new(PosColumnKind::PosSize),
                    PosTableColumn::new(PosColumnKind::EntryPrice),
                    PosTableColumn::new(PosColumnKind::Breakeven),
                    PosTableColumn::new(PosColumnKind::MarkPrice),
                    PosTableColumn::new(PosColumnKind::LiqPrice),
                    PosTableColumn::new(PosColumnKind::MarginAmt),
                    PosTableColumn::new(PosColumnKind::UnrealPnL),
                ],
                position_rows: vec![],
                account_info_usdt: None,
            },
            Command::batch(vec![
                font::load(ICON_BYTES).map(Message::FontLoaded),

                if API_KEY != "" && SECRET_KEY != "" {
                    Command::perform(user_data::get_listen_key(API_KEY, SECRET_KEY), |res| {
                        match res {
                            Ok(listen_key) => {
                                Message::UserKeySucceed(listen_key)
                            },
                            Err(err) => {
                                dbg!(err);
                                Message::UserKeyError
                            }
                        }
                    })
                } else {
                    eprintln!("API keys not set");
                    Command::none()
                },
                Command::perform(
                    async move {
                        (pane_grid::Axis::Horizontal, first_pane) 
                    },
                    move |(axis, pane)| {
                        Message::Split(axis, pane, PaneId::HeatmapChart)
                    }
                ),
            ]),
        )
    }

    fn title(&self) -> String {
        "Iced Trade".to_owned()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::CustomLine(message) => {
                self.custom_line.update(message);
                Command::none()
            },

            Message::TickerSelected(ticker) => {
                self.selected_ticker = Some(ticker);
                Command::none()
            },
            Message::TimeframeSelected(timeframe) => {
                self.selected_timeframe = Some(timeframe);
                Command::none()
            },
            Message::ExchangeSelected(exchange) => {
                self.selected_exchange = Some(exchange);
                Command::none()
            },
            Message::WsToggle() => {
                self.ws_running =! self.ws_running;

                if self.ws_running {
                    let selected_ticker = match &self.selected_ticker {
                        Some(ticker) => ticker,
                        None => {
                            eprintln!("No ticker selected");
                            self.ws_running = false;
                            return Command::none();
                        }
                    };
                    let selected_timeframe = match &self.selected_timeframe {
                        Some(timeframe) => timeframe,
                        None => {
                            eprintln!("No timeframe selected");
                            self.ws_running = false;
                            return Command::none();
                        }
                    };
            
                    let fetch_klines = Command::perform(
                        market_data::fetch_klines(selected_ticker.to_string(), selected_timeframe.to_string())
                            .map_err(|err| format!("{}", err)), 
                        |klines| {
                            Message::FetchEvent(klines)
                        }
                    );
                    let mut commands = vec![fetch_klines];

                    if let Some(_listen_key) = &self.listen_key {
                        let fetch_open_orders = Command::perform(
                            user_data::fetch_open_orders(selected_ticker.to_string(), API_KEY, SECRET_KEY)
                                .map_err(|err| format!("{:?}", err)),
                            |orders| {
                                match orders {
                                    Ok(orders) => {
                                        Message::OrdersFetched(orders)
                                    },
                                    Err(err) => {
                                        Message::OrderFailed(format!("{}", err))
                                    }
                                }
                            }
                        );
                        let fetch_open_positions = Command::perform(
                            user_data::fetch_open_positions(API_KEY, SECRET_KEY)
                                .map_err(|err| format!("{:?}", err)),
                            |positions| {
                                match positions {
                                    Ok(positions) => {
                                        Message::UserWsEvent(user_data::Event::FetchedPositions(positions))
                                    },
                                    Err(err) => {
                                        Message::OrderFailed(format!("{}", err))
                                    }
                                }
                            }
                        );
                        let fetch_balance = Command::perform(
                            user_data::fetch_acc_balance(API_KEY, SECRET_KEY)
                                .map_err(|err| format!("{:?}", err)),
                            |balance| {
                                match balance {
                                    Ok(balance) => {
                                        let mut message = Message::OrderFailed("No USDT balance found".to_string());
                                        for asset in balance {
                                            if asset.asset == "USDT" {
                                                message = Message::UpdateAccInfo(asset);
                                                break;
                                            }
                                        }
                                        message
                                    },
                                    Err(err) => {
                                        Message::OrderFailed(format!("{}", err))
                                    }
                                }
                            }
                        );
                        commands.extend(vec![fetch_open_orders, fetch_open_positions, fetch_balance]);
                    } else {
                        eprintln!("No listen key found for user data fetch");
                    }

                    let first_pane = self.first_pane;

                    for (pane_id, is_open) in &self.panes_open {
                        if *is_open {
                            if !self.panes.panes.values().any(|pane| pane.id == *pane_id) {
                                let pane_id = *pane_id;
                                let split_pane = Command::perform(
                                    async move {
                                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                        (pane_grid::Axis::Horizontal, first_pane) 
                                    },
                                    move |(axis, pane)| {
                                        Message::Split(axis, pane, pane_id)
                                    }
                                );
                                commands.push(split_pane);
                            }
                            if *pane_id == PaneId::HeatmapChart {
                                if self.trades_chart.is_none() {
                                    self.trades_chart = Some(LineChart::new());
                                }
                            }
                            if *pane_id == PaneId::TimeAndSales {
                                if self.time_and_sales.is_none() {
                                    self.time_and_sales = Some(TimeAndSales::new());
                                }
                            }
                        }
                    }

                    Command::batch(commands)

                } else {
                    self.ws_state = WsState::Disconnected;

                    self.trades_chart = None;
                    self.candlestick_chart = None;
                    self.time_and_sales = None;

                    self.open_orders.clear();
                    self.orders_rows.clear();
                    self.position_rows.clear();

                    Command::none()
                }
            },       
            Message::FetchEvent(klines) => {
                match klines {
                    Ok(klines) => {
                        let klines_clone = klines.clone(); // Clone klines
                        let timeframe_in_minutes = match &self.selected_timeframe {
                            Some(timeframe) => {
                                match timeframe {
                                    Timeframe::M1 => 1,
                                    Timeframe::M3 => 3,
                                    Timeframe::M5 => 5,
                                    Timeframe::M15 => 15,
                                    Timeframe::M30 => 30,
                                }
                            },
                            None => {
                                eprintln!("No timeframe selected");
                                return Command::none();
                            }
                        };

                        self.candlestick_chart = Some(CandlestickChart::new(klines, timeframe_in_minutes));

                        self.custom_line.set_dataset(klines_clone);
                    },
                    Err(err) => {
                        eprintln!("Error fetching klines: {}", err);
                        self.candlestick_chart = Some(CandlestickChart::new(vec![], 1));
                    },
                }
                Command::none()
            },
            Message::MarketWsEvent(event) => {
                match event {
                    market_data::Event::Connected(connection) => {
                        self.ws_state = WsState::Connected(connection);
                    }
                    market_data::Event::Disconnected => {
                        self.ws_state = WsState::Disconnected;
                    }
                    market_data::Event::DepthReceived(depth_update, bids, asks, trades_buffer) => {
                        if let Some(time_and_sales) = &mut self.time_and_sales {
                            time_and_sales.update(&trades_buffer);
                        } 
                        if let Some(chart) = &mut self.trades_chart {
                            chart.update(depth_update, trades_buffer, bids, asks);
                        } 
                    }
                    market_data::Event::KlineReceived(kline) => {
                        let kline_clone = kline.clone();

                        if let Some(chart) = &mut self.candlestick_chart {
                            chart.update(kline);
                        }
                        
                        self.custom_line.insert_datapoint(kline_clone);
                    }
                };
                Command::none()
            },
            Message::UserWsEvent(event) => {
                match event {
                    user_data::Event::Connected(connection) => {
                        self.user_ws_state = UserWsState::Connected(connection);
                    }
                    user_data::Event::Disconnected => {
                        self.user_ws_state = UserWsState::Disconnected;
                    }
                    user_data::Event::CancelOrder(order_trade_update) => {
                        TableRow::remove_row(order_trade_update.order_id, &mut self.orders_rows);
                    }
                    user_data::Event::NewOrder(order) => {
                        dbg!(order);
                    }
                    user_data::Event::TestEvent(msg) => {
                        dbg!(msg);
                    }
                    user_data::Event::NewPositions(positions) => {
                        for position in positions {
                            PosTableRow::remove_row(&position.symbol, &mut self.position_rows);
                            if position.pos_amt != 0.0 {
                                let position_in_table = user_data::PositionInTable { 
                                    symbol: position.symbol.clone(),
                                    size: position.pos_amt,
                                    entry_price: position.entry_price,
                                    breakeven_price: position.breakeven_price,
                                    mark_price: 0.0, 
                                    liquidation_price: 0.0,
                                    margin_amt: 0.0, 
                                    unrealized_pnl: 0.0,
                                };

                                self.position_rows.push(PosTableRow::add_row(position_in_table));
                            }
                        }
                    }
                    user_data::Event::FetchedPositions(positions) => {
                        self.position_rows.clear();
                    
                        for fetched_position in positions {
                            if fetched_position.pos_amt != 0.0 {
                                let position_in_table = user_data::PositionInTable { 
                                    symbol: fetched_position.symbol.clone(),
                                    size: fetched_position.pos_amt,
                                    entry_price: fetched_position.entry_price,
                                    breakeven_price: fetched_position.breakeven_price,
                                    mark_price: fetched_position.mark_price,
                                    liquidation_price: fetched_position.liquidation_price,
                                    margin_amt: 0.0,
                                    unrealized_pnl: fetched_position.unrealized_pnl,
                                };
                    
                                self.position_rows.push(PosTableRow::add_row(position_in_table));
                            }
                        }
                    }
                    user_data::Event::FetchedBalance(balance) => {
                        for asset in balance {
                            if asset.asset == "USDT" {
                                self.account_info_usdt = Some(asset);
                                break;
                            }
                        }
                    }
                };
                Command::none()
            },
            Message::UserKeySucceed(listen_key) => {
                self.listen_key = Some(listen_key);
                Command::none()
            },
            Message::UserKeyError => {
                eprintln!("Check API keys");
                Command::none()
            },

            // Pane grid
            Message::Split(axis, pane, pane_id) => {
                let focus_pane = if let Some((pane, _)) = self.panes.split(axis, pane, Pane::new(pane_id)) {
                    Some(pane)
                } else if let Some((&first_pane, _)) = self.panes.panes.iter().next() {
                    self.panes.split(axis, first_pane, Pane::new(pane_id)).map(|(pane, _)| pane)
                } else {
                    None
                };

                if Some(focus_pane) != None {
                    self.focus = focus_pane;
                    self.panes_open.insert(pane_id, true);

                    if pane_id == PaneId::TimeAndSales {
                        self.time_and_sales = Some(TimeAndSales::new());
                    }
                    if pane_id == PaneId::HeatmapChart {
                        self.trades_chart = Some(LineChart::new());
                    }
                } 

                Command::none()
            },
            Message::Clicked(pane) => {
                self.focus = Some(pane);
                Command::none()
            },
            Message::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                Command::none()
            },
            Message::Dragged(pane_grid::DragEvent::Dropped {
                pane,
                target,
            }) => {
                self.panes.drop(pane, target);
                Command::none()
            },
            Message::Dragged(_) => {
                Command::none()
            },
            Message::Maximize(pane) => {
                self.panes.maximize(pane);
                Command::none()
            },
            Message::Restore => {
                self.panes.restore();
                Command::none()
            },
            Message::Close(pane) => {
                self.panes.get(pane).map(|pane| {
                    match pane.id {
                        PaneId::HeatmapChart => {
                            self.panes_open.insert(PaneId::HeatmapChart, false);
                            self.trades_chart = None;
                        },
                        PaneId::CandlestickChart => {
                            self.panes_open.insert(PaneId::CandlestickChart, false);
                        },
                        PaneId::TimeAndSales => {
                            self.panes_open.insert(PaneId::TimeAndSales, false);
                            self.time_and_sales = None;
                        },
                        PaneId::TradePanel => {
                            self.panes_open.insert(PaneId::TradePanel, false);
                        },  
                    }
                });
                
                if let Some((_, sibling)) = self.panes.close(pane) {
                    self.focus = Some(sibling);
                }
                Command::none()
            },
            Message::ToggleLayoutLock => {
                self.focus = None;
                self.pane_lock = !self.pane_lock;
                self.resize_columns_enabled = !self.pane_lock;
                Command::none()
            },

            // Order form
            Message::LimitOrder(side) => {
                Command::perform(
                    user_data::create_limit_order(side, self.qty_input_val.borrow().as_ref().unwrap().to_string(), self.price_input_val.borrow().as_ref().unwrap().to_string(), API_KEY, SECRET_KEY),
                    |res| {
                        match res {
                            Ok(res) => {
                                Message::OrderCreated(res)
                            },
                            Err(user_data::BinanceError::Reqwest(err)) => {
                                Message::OrderFailed(format!("Network error: {}", err))
                            },
                            Err(user_data::BinanceError::BinanceAPI(err_msg)) => {
                                Message::OrderFailed(format!("Binance API error: {}", err_msg))
                            }
                        }
                    }
                )
            },
            Message::MarketOrder(side) => {
                Command::perform(
                    user_data::create_market_order(side, self.qty_input_val.borrow().as_ref().unwrap().to_string(), API_KEY, SECRET_KEY),
                    |res| {
                        match res {
                            Ok(res) => {
                                Message::MarketOrderCreated(res)
                            },
                            Err(user_data::BinanceError::Reqwest(err)) => {
                                Message::OrderFailed(format!("Network error: {}", err))
                            },
                            Err(user_data::BinanceError::BinanceAPI(err_msg)) => {
                                Message::OrderFailed(format!("Binance API error: {}", err_msg))
                            }
                        }
                    }
                )
            },
            Message::CancelOrder(order_id) => {
                Command::perform(
                    user_data::cancel_order(order_id, API_KEY, SECRET_KEY),
                    |res| {
                        match res {
                            Ok(_) => {
                                Message::OrderFailed("Order cancelled".to_string())
                            },
                            Err(user_data::BinanceError::Reqwest(err)) => {
                                Message::OrderFailed(format!("Network error: {}", err))
                            },
                            Err(user_data::BinanceError::BinanceAPI(err_msg)) => {
                                Message::OrderFailed(format!("Binance API error: {}", err_msg))
                            }
                        }
                    }
                )
            },
            Message::OrdersFetched(orders) => {
                for order in orders {
                    self.open_orders.push(order.clone());
                    self.orders_rows.push(TableRow::add_row(order));
                }
                Command::none()
            },
            Message::OrderCreated(order) => {
                self.orders_rows.push(TableRow::add_row(order.clone()));
                self.open_orders.push(order);
                Command::none()
            },
            Message::MarketOrderCreated(order) => {
                dbg!(order);
                Command::none()
            },
            Message::OrderFailed(err) => {
                eprintln!("Error creating order: {}", err);
                Command::none()
            },

            Message::InputChanged((field, new_value)) => {
                if field == "price" {
                    *self.price_input_val.borrow_mut() = Some(new_value);
                } else if field == "qty" {
                    *self.qty_input_val.borrow_mut() = Some(new_value);
                }
                Command::none()
            },
            Message::UpdateAccInfo(acc_info) => {
                self.account_info_usdt = Some(acc_info);
                Command::none()
            },

            // Table 
            Message::SyncHeader(offset) => {
                let orders_batch = Command::batch(vec![
                    scrollable::scroll_to(self.orders_header.clone(), offset),
                    scrollable::scroll_to(self.orders_footer.clone(), offset),
                ]);
                let positions_batch = Command::batch(vec![
                    scrollable::scroll_to(self.pos_header.clone(), offset),
                    scrollable::scroll_to(self.pos_footer.clone(), offset),
                ]);

                if self.table_active_tab == 0 {
                    orders_batch
                } else if self.table_active_tab == 1 {
                    positions_batch
                } else {
                    Command::none()
                }
            },
            Message::TableResizing(index, offset) => {
                if self.table_active_tab == 0 {
                    self.orders_columns[index].resize_offset = Some(offset);
                } else if self.table_active_tab == 1 {
                    self.position_columns[index].resize_offset = Some(offset);
                }
                Command::none()
            },
            Message::TableResized => {
                if self.table_active_tab == 0 {
                    self.orders_columns.iter_mut().for_each(|column| {
                        if let Some(offset) = column.resize_offset.take() {
                            column.width += offset;
                        }
                    });
                } else if self.table_active_tab == 1 {
                    self.position_columns.iter_mut().for_each(|column| {
                        if let Some(offset) = column.resize_offset.take() {
                            column.width += offset;
                        }
                    });
                }
                Command::none()
            },
            Message::FooterEnabled(enabled) => {
                self.footer_enabled = enabled;
                Command::none()
            },
            Message::MinWidthEnabled(enabled) => {
                self.min_width_enabled = enabled;
                Command::none()
            },
            Message::TabSelected(index, tab_type) => {
                if tab_type == "order_form" {
                    self.order_form_active_tab = index;
                } else if tab_type == "table" {
                    self.table_active_tab = index;
                }
                Command::none()
            },

            Message::Debug(_msg) => {
                Command::none()
            },
            Message::FontLoaded(_) => {
                dbg!("Font loaded");
                Command::none()
            },

            Message::OpenModal(pane) => {
                self.panes.get_mut(pane).map(|pane| {
                    pane.show_modal = true;
                });
                Command::none()
            },
            Message::CloseModal => {
                for pane in self.panes.panes.values_mut() {
                    pane.show_modal = false;
                }
                Command::none()
            },

            Message::SliderChanged(pane_id, value) => {
                if pane_id == PaneId::TimeAndSales {
                    self.size_filter_timesales = value;
                    if self.sync_heatmap {
                        self.size_filter_heatmap = value;
                    }
                } else if pane_id == PaneId::HeatmapChart {
                    self.size_filter_heatmap = value;
                    self.sync_heatmap = false;
                }

                self.trades_chart.as_mut().map(|chart| {
                    chart.set_size_filter(self.size_filter_heatmap);
                });
                self.time_and_sales.as_mut().map(|time_and_sales| {
                    time_and_sales.set_size_filter(self.size_filter_timesales);
                });

                Command::none()
            },
            Message::SyncWithHeatmap(sync) => {
                self.sync_heatmap = sync;
            
                if sync {
                    self.size_filter_heatmap = self.size_filter_timesales;
                    self.trades_chart.as_mut().map(|chart| {
                        chart.set_size_filter(self.size_filter_heatmap);
                    });
                }
            
                Command::none()
            },
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let focus = self.focus;
        let total_panes = self.panes.len();

        let pane_grid = PaneGrid::new(&self.panes, |id, pane, is_maximized| {
            let is_focused = focus == Some(id);
    
            let content: pane_grid::Content<'_, Message, _, Renderer> = pane_grid::Content::new(responsive(move |size| {
                view_content(
                    pane.id, 
                    pane.show_modal,
                    &self.size_filter_heatmap,
                    &self.size_filter_timesales,
                    self.sync_heatmap,
                    total_panes, 
                    size, 
                    &self.time_and_sales,
                    &self.trades_chart, 
                    &self.candlestick_chart, 
                    &self.custom_line,
                    self.qty_input_val.borrow().clone(), 
                    self.price_input_val.borrow().clone(),
                    &self.orders_header,
                    &self.orders_body,
                    &self.pos_header,
                    &self.pos_body,
                    &self.orders_columns,
                    &self.orders_rows,
                    &self.position_columns,
                    &self.position_rows,
                    &self.min_width_enabled,
                    &self.resize_columns_enabled,
                    &self.order_form_active_tab,
                    &self.table_active_tab,
                    &self.account_info_usdt,
                )
            }));
    
            if self.pane_lock {
                return content.style(style::pane_active);
            }
    
            let mut content = content.style(if is_focused {
                style::pane_focused
            } else {
                style::pane_active
            });
    
            let title = match pane.id {
                PaneId::HeatmapChart => "Heatmap Chart",
                PaneId::CandlestickChart => "Candlestick Chart",
                PaneId::TimeAndSales => "Time & Sales",
                PaneId::TradePanel => "Trading Panel",
            };            
            if is_focused {
                let title_bar = pane_grid::TitleBar::new(title)
                    .controls(view_controls(
                        id,
                        total_panes,
                        is_maximized,
                    ))
                    .padding(4)
                    .style(style::title_bar_focused);
    
                content = content.title_bar(title_bar);
            }
            content
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(10)
        .on_click(Message::Clicked)
        .on_drag(Message::Dragged)
        .on_resize(10, Message::Resized);

        let layout_lock_button = button(
            container(
                if self.pane_lock { 
                    text(char::from(Icon::Locked).to_string()).font(ICON) 
                } else { 
                    text(char::from(Icon::Unlocked).to_string()).font(ICON) 
                })
                .center_x().width(25)
            )
            .on_press(Message::ToggleLayoutLock);

        let add_pane_button = button(
            container(
                text(char::from(Icon::Layout).to_string()).font(ICON))
                .center_x().width(25)
            )
            .on_press(Message::Debug("Add Pane".to_string()));

        let menu_tpl_1 = |items| Menu::new(items).max_width(180.0).offset(15.0).spacing(5.0);
        let mb = menu_bar!(
            (add_pane_button, {
                menu_tpl_1(menu_items!(
                    (debug_button(PaneId::HeatmapChart, self.panes_open.get(&PaneId::HeatmapChart).unwrap_or(&false), self.first_pane))
                    (debug_button(PaneId::CandlestickChart, self.panes_open.get(&PaneId::CandlestickChart).unwrap_or(&false), self.first_pane))
                    (debug_button(PaneId::TimeAndSales, self.panes_open.get(&PaneId::TimeAndSales).unwrap_or(&false), self.first_pane))
                    (debug_button(PaneId::TradePanel, self.panes_open.get(&PaneId::TradePanel).unwrap_or(&false), self.first_pane))
                )).width(200.0)
            })
        );

        let ws_button = if self.selected_ticker.is_some() {
            button(if self.ws_running { "Disconnect" } else { "Connect" })
                .on_press(Message::WsToggle())
        } else {
            button(if self.ws_running { "Disconnect" } else { "Connect" })
        };
        let mut ws_controls = Row::new()
            .spacing(10)
            .align_items(Alignment::Center)
            .push(ws_button);

        if !self.ws_running {
            let symbol_pick_list = pick_list(
                &Ticker::ALL[..],
                self.selected_ticker,
                Message::TickerSelected,
            ).placeholder("Choose a ticker...");
            
            let timeframe_pick_list = pick_list(
                &Timeframe::ALL[..],
                self.selected_timeframe,
                Message::TimeframeSelected,
            );
            let exchange_selector = pick_list(
                &["Binance Futures"][..],
                self.selected_exchange,
                Message::ExchangeSelected,
            ).placeholder("Choose an exchange...");
        
            ws_controls = ws_controls
                .push(exchange_selector)
                .push(symbol_pick_list)
                .push(timeframe_pick_list);
                
        } else {
            ws_controls = ws_controls.push(Text::new(self.selected_ticker.unwrap_or_else(|| { dbg!("No ticker found"); Ticker::BTCUSDT } ).to_string()).size(20));
        }

        let content = Column::new()
            .spacing(10)
            .align_items(Alignment::Start)
            .width(Length::Fill)
            .height(Length::Fill)
            .push(
                Row::new()
                    .spacing(10)
                    .push(ws_controls)
                    .push(Space::with_width(Length::Fill))
                    .push(mb)                
                    .push(
                        tooltip(layout_lock_button, "Layout Lock", tooltip::Position::Bottom).style(theme::Container::Box)
                    )
            )
            .push(pane_grid);

        Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(10)
            .center_x()
            .center_y()
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();
    
        if self.ws_running {
            self.selected_ticker.and_then(|ticker| {
                self.selected_timeframe.map(|timeframe| {
                    let binance_market_stream = market_data::connect_market_stream(ticker, timeframe).map(Message::MarketWsEvent);
                    subscriptions.push(binance_market_stream);
                })
            });
        }
        if let Some(listen_key) = &self.listen_key {
            let binance_user_stream = user_data::connect_user_stream(listen_key.to_string()).map(Message::UserWsEvent);
            subscriptions.push(binance_user_stream);

            let fetch_positions = user_data::fetch_user_stream(API_KEY, SECRET_KEY).map(Message::UserWsEvent);
            subscriptions.push(fetch_positions);
        }
        
        Subscription::batch(subscriptions)
    }    

    fn theme(&self) -> Self::Theme {
        Theme::Oxocarbon
    }
}

fn debug_button<'a>(label: PaneId, is_open: &bool, pane_to_split: pane_grid::Pane) -> button::Button<'a, Message, iced::Theme, iced::Renderer> {
    if *is_open {
        disabled_labeled_button(&format!("{:?}", label))
    } else {
        labeled_button(&format!("{:?}", label), Message::Split(pane_grid::Axis::Vertical, pane_to_split, label))
    }
}
fn labeled_button<'a>(
    label: &str,
    msg: Message,
) -> button::Button<'a, Message, iced::Theme, iced::Renderer> {
    base_button(
        text(label).vertical_alignment(alignment::Vertical::Center),
        msg,
    )
}
fn disabled_labeled_button<'a>(
    label: &str,
) -> button::Button<'a, Message, iced::Theme, iced::Renderer> {
    let content = text(label)
        .vertical_alignment(alignment::Vertical::Center);
    button(content)
        .padding([4, 8])
        .width(150)
}
fn base_button<'a>(
    content: impl Into<Element<'a, Message, iced::Theme, iced::Renderer>>,
    msg: Message,
) -> button::Button<'a, Message, iced::Theme, iced::Renderer> {
    button(content)
        .padding([4, 8])
        .width(150)
        .on_press(msg)
}

fn view_content<'a, 'b: 'a>(
    pane_id: PaneId,
    show_modal: bool,
    size_filter_heatmap: &'a f32,
    size_filter_timesales: &'a f32,
    sync_heatmap: bool,
    _total_panes: usize,
    _size: Size,
    time_and_sales: &'a Option<TimeAndSales>,
    trades_chart: &'a Option<LineChart>,
    candlestick_chart: &'a Option<CandlestickChart>,
    custom_line: &'a CustomLine,
    qty_input_val: Option<String>,
    price_input_val: Option<String>, 
    orders_header: &'b scrollable::Id,
    orders_body: &'b scrollable::Id,
    pos_header: &'b scrollable::Id,
    pos_body: &'b scrollable::Id,
    orders_columns: &'b Vec<TableColumn>,
    orders_rows: &'b Vec<TableRow>,
    position_columns: &'b Vec<PosTableColumn>,
    position_rows: &'b Vec<PosTableRow>,
    min_width_enabled: &'b bool,
    resize_columns_enabled: &'b bool,
    order_form_active_tab: &'b usize,
    table_active_tab: &'b usize,
    account_info_usdt: &'b Option<user_data::FetchedBalance>,
) -> Element<'a, Message> {
    let content: Element<Message, Theme, Renderer> = match pane_id {
        PaneId::HeatmapChart => {
            let underlay = trades_chart.as_ref().map(LineChart::view).unwrap_or_else(|| Text::new("No data").into());
            let overlay = if show_modal {
                Some(
                    Card::new(
                        Text::new("Heatmap Chart -> Settings"),
                        Column::new()
                            .push(Text::new("Size Filtering"))
                            .push(
                                Slider::new(0.0..=50000.0, *size_filter_heatmap, move |value| Message::SliderChanged(PaneId::HeatmapChart, value))
                                    .step(500.0)
                            ),
                    )
                    .foot(
                        Row::new()
                            .spacing(10)
                            .padding(5)
                            .width(Length::Fill)
                            .push(
                                Text::new(format!("${}", size_filter_heatmap)).size(16)
                            )
                    )
                    .max_width(500.0)
                    .on_close(Message::CloseModal)
                )
            } else {
                None
            };

            modal(underlay, overlay)
                .backdrop(Message::CloseModal)
                .on_esc(Message::CloseModal)
                .align_y(alignment::Vertical::Center)
                .into()
        }, 
        
        PaneId::CandlestickChart => { 
            let underlay = candlestick_chart.as_ref().map(CandlestickChart::view).unwrap_or_else(|| Text::new("No data").into());
            let overlay = if show_modal {
                Some(
                    Card::new(
                        Text::new("Candlestick Chart -> Settings"),
                        Column::new()
                            .push(Text::new("Test"))
                    )
                    .foot(
                        Row::new()
                            .spacing(10)
                            .padding(5)
                            .width(Length::Fill)
                            .push(
                                Text::new("Footer").size(16)
                            )
                    )
                    .max_width(500.0)
                    .on_close(Message::CloseModal)
                )
            } else {
                None
            };

            modal(underlay, overlay)
                .backdrop(Message::CloseModal)
                .on_esc(Message::CloseModal)
                .align_y(alignment::Vertical::Center)
                .into()
        },
        
        PaneId::TimeAndSales => { 
            let underlay = time_and_sales.as_ref().map(TimeAndSales::view).unwrap_or_else(|| Text::new("No data").into()); 
            let overlay = if show_modal {
                Some(
                    Card::new(
                        Text::new("Time & Sales -> Settings"),
                        Column::new()
                            .push(Text::new("Size Filtering"))
                            .push(
                                Slider::new(0.0..=50000.0, *size_filter_timesales, move |value| Message::SliderChanged(PaneId::TimeAndSales, value))
                                    .step(500.0)
                            ),
                    )
                    .foot(
                        Row::new()
                            .spacing(10)
                            .padding(5)
                            .width(Length::Fill)
                            .push(
                                Text::new(format!("${}", size_filter_timesales)).size(16)
                            )
                            .push(Space::with_width(Length::Fill))
                            .push(
                                checkbox("Sync Heatmap with", sync_heatmap)
                                    .on_toggle(Message::SyncWithHeatmap)
                            )
                    )
                    .max_width(500.0)
                    .on_close(Message::CloseModal)
                )
            } else {
                None
            };

            modal(underlay, overlay)
                .backdrop(Message::CloseModal)
                .on_esc(Message::CloseModal)
                .align_y(alignment::Vertical::Center)
                .into()
        },  
        
        PaneId::TradePanel => if account_info_usdt.is_none() {
            custom_line
                .view()
                .map(move |message| Message::CustomLine(message))
                .into()
        } else {
            let form_select_0_button = button("Market Order")
                .on_press(Message::TabSelected(0, "order_form".to_string()));
            let form_select_1_button = button("Limit Order") 
                .on_press(Message::TabSelected(1, "order_form".to_string()));

            let (buy_button, sell_button) = match *order_form_active_tab {
                0 => {
                    (
                        button("Limit Buy").on_press(Message::LimitOrder("BUY".to_string())),
                        button("Limit Sell").on_press(Message::LimitOrder("SELL".to_string()))
                    )
                },
                1 => {
                    (
                        button("Market Buy").on_press(Message::MarketOrder("BUY".to_string())),
                        button("Market Sell").on_press(Message::MarketOrder("SELL".to_string()))
                    )
                },
                _ => {
                    (
                        button("Buy").on_press(Message::LimitOrder("BUY".to_string())),
                        button("Sell").on_press(Message::LimitOrder("SELL".to_string()))
                    )
                },
            };
            let order_buttons = Row::new()
                .push(buy_button)
                .push(sell_button)
                .align_items(Alignment::Center)
                .spacing(5);
        
            let qty_input = text_input("Quantity...", qty_input_val.as_deref().unwrap_or(""))
                .on_input(|input| Message::InputChanged(("qty".to_string(), input)));
        
            let inputs = if *order_form_active_tab == 0 {
                let price_input = text_input("Price...", price_input_val.as_deref().unwrap_or(""))
                    .on_input(|input| Message::InputChanged(("price".to_string(), input)));
        
                Row::new()
                    .push(form_select_1_button)
                    .push(price_input)
                    .push(qty_input)                       
                    .push(order_buttons)
                    .align_items(Alignment::Center)
                    .padding([20, 10])
                    .spacing(5)
            } else {
                Row::new()
                    .push(form_select_0_button)
                    .push(qty_input)
                    .push(order_buttons)
                    .align_items(Alignment::Center)
                    .padding([20, 10])
                    .spacing(5)
            };

            if *table_active_tab == 0 {
                let table = responsive(move |size| {
                    let mut table = table(
                        orders_header.clone(),
                        orders_body.clone(),
                        &orders_columns,
                        &orders_rows,
                        Message::SyncHeader,
                    );
                    if *min_width_enabled { table = table.min_width(size.width); }
                    if *resize_columns_enabled {
                        table = table.on_column_resize(Message::TableResizing, Message::TableResized);
                    }
            
                    Container::new(table).padding(10).into()
                });
                Column::new()
                    .push(inputs)
                    .push(
                        Row::new()
                            .push(
                                button("Positions")
                                .on_press(Message::TabSelected(1, "table".to_string()))
                            )
                            .push(
                                button("Open Orders")
                            )
                            .push(Space::with_width(Length::Fill)) 
                            .push(account_info_usdt.as_ref().map(|info| {
                                Text::new(format!("USDT: {:.2}", info.balance))
                            }).unwrap_or_else(|| Text::new("").size(16)))
                            .padding([0, 10, 0, 10])
                    )
                    .push(table)
                    .align_items(Alignment::Center)
                    .into()
            } else {
                let table = responsive(move |size| {
                    let mut table = table(
                        pos_header.clone(),
                        pos_body.clone(),
                        &position_columns,
                        &position_rows,
                        Message::SyncHeader,
                    );
                    if *min_width_enabled { table = table.min_width(size.width); }
                    if *resize_columns_enabled {
                        table = table.on_column_resize(Message::TableResizing, Message::TableResized);
                    }
            
                    Container::new(table).padding(10).into()
                });
                Column::new()
                    .push(inputs)
                    .push(
                        Row::new()
                            .push(
                                button("Positions")
                            )
                            .push(
                                button("Open Orders")
                                .on_press(Message::TabSelected(0, "table".to_string()))
                            )
                            .push(Space::with_width(Length::Fill)) 
                            .push(account_info_usdt.as_ref().map(|info| {
                                Text::new(format!("USDT: {:.2}", info.balance))
                            }).unwrap_or_else(|| Text::new("").size(16)))
                            .padding([0, 10, 0, 10])
                    )
                    .push(table)
                    .align_items(Alignment::Center)
                    .into()
            }        
        },
    };

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_controls<'a>(
    pane: pane_grid::Pane,
    total_panes: usize,
    is_maximized: bool,
) -> Element<'a, Message> {
    let mut row = row![].spacing(5);

    if total_panes > 1 {
        let (icon, message) = if is_maximized {
            (Icon::ResizeSmall, Message::Restore)
        } else {
            (Icon::ResizeFull, Message::Maximize(pane))
        };
        let buttons = vec![
            (container(text(char::from(Icon::Cog).to_string()).font(ICON).size(14)).width(25).center_x(), Message::OpenModal(pane)),
            (container(text(char::from(icon).to_string()).font(ICON).size(14)).width(25).center_x(), message),
            (container(text(char::from(Icon::Close).to_string()).font(ICON).size(14)).width(25).center_x(), Message::Close(pane)),
        ];

        for (content, message) in buttons {        
            row = row.push(
                button(content)
                    .padding(3)
                    .on_press(message),
            );
        }
    }
    row.into()
}

use crate::market_data::Trade;
struct ConvertedTrade {
    time: NaiveDateTime,
    price: f32,
    qty: f32,
    is_sell: bool,
}
struct TimeAndSales {
    recent_trades: Vec<ConvertedTrade>,
    size_filter: f32,
}
impl TimeAndSales {
    fn new() -> Self {
        Self {
            recent_trades: Vec::new(),
            size_filter: 0.0,
        }
    }
    fn set_size_filter(&mut self, value: f32) {
        self.size_filter = value;
    }

    fn update(&mut self, trades_buffer: &Vec<Trade>) {
        for trade in trades_buffer {
            let trade_time = NaiveDateTime::from_timestamp(trade.time as i64 / 1000, (trade.time % 1000) as u32 * 1_000_000);
            let converted_trade = ConvertedTrade {
                time: trade_time,
                price: trade.price,
                qty: trade.qty,
                is_sell: trade.is_sell,
            };
            self.recent_trades.push(converted_trade);
        }

        if self.recent_trades.len() > 2000 {
            let drain_to = self.recent_trades.len() - 2000;
            self.recent_trades.drain(0..drain_to);
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let mut trades_column = Column::new()
            .height(Length::Fill)
            .padding(10);

        let filtered_trades: Vec<&ConvertedTrade> = self.recent_trades.iter().filter(|trade| (trade.qty*trade.price) >= self.size_filter).collect();

        let max_qty = filtered_trades.iter().map(|trade| trade.qty).fold(0.0, f32::max);
    
        if filtered_trades.is_empty() {
            trades_column = trades_column.push(Text::new("No trades").size(16));
        } else {
            for trade in filtered_trades.iter().rev().take(80) {
                let trade_row = Row::new()
                    .push(
                        container(Text::new(format!("{}", trade.time.format("%M:%S.%3f"))).size(14))
                            .width(Length::FillPortion(8)).center_x()
                    )
                    .push(
                        container(Text::new(format!("{}", trade.price)).size(14))
                            .width(Length::FillPortion(6))
                    )
                    .push(
                        container(Text::new(if trade.is_sell { "Sell" } else { "Buy" }).size(14))
                            .width(Length::FillPortion(4))
                    )
                    .push(
                        container(Text::new(format!("{}", trade.qty)).size(14))
                            .width(Length::FillPortion(4))
                    );

                let color_alpha = trade.qty / max_qty;
    
                trades_column = trades_column.push(container(trade_row)
                    .style(if trade.is_sell { style::sell_side_red(color_alpha) } else { style::buy_side_green(color_alpha) }));
    
                trades_column = trades_column.push(Container::new(Space::new(Length::Fixed(0.0), Length::Fixed(5.0))));
            }
        }
    
        trades_column.into()  
    }    
}

mod style {
    use iced::widget::container;
    use iced::{Border, Color, Theme};

    pub fn title_bar_active(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            text_color: Some(palette.background.strong.text),
            background: Some(palette.background.strong.color.into()),
            ..Default::default()
        }
    }
    pub fn title_bar_focused(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            text_color: Some(palette.primary.strong.text),
            background: Some(palette.primary.strong.color.into()),
            ..Default::default()
        }
    }
    pub fn pane_active(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            background: Some(Color::BLACK.into()),
            border: Border {
                width: 1.0,
                color: palette.background.strong.color,
                ..Border::default()
            },
            ..Default::default()
        }
    }
    pub fn pane_focused(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            background: Some(Color::BLACK.into()),
            border: Border {
                width: 1.0,
                color: palette.primary.strong.color,
                ..Border::default()
            },
            ..Default::default()
        }
    }
    pub fn sell_side_red(color_alpha: f32) -> container::Appearance {
        //let palette = theme.extended_palette();

        container::Appearance {
            text_color: Color::from_rgba(192.0 / 255.0, 80.0 / 255.0, 77.0 / 255.0, 1.0).into(),
            border: Border {
                width: 1.0,
                color: Color::from_rgba(192.0 / 255.0, 80.0 / 255.0, 77.0 / 255.0, color_alpha).into(),
                ..Border::default()
            },
            ..Default::default()
        }
    }
    pub fn buy_side_green(color_alpha: f32) -> container::Appearance {
        //let palette = theme.extended_palette();

        container::Appearance {
            text_color: Color::from_rgba(81.0 / 255.0, 205.0 / 255.0, 160.0 / 255.0, 1.0).into(),
            border: Border {
                width: 1.0,
                color: Color::from_rgba(81.0 / 255.0, 205.0 / 255.0, 160.0 / 255.0, color_alpha).into(),
                ..Border::default()
            },
            ..Default::default()
        }
    }
}
struct TableColumn {
    kind: ColumnKind,
    width: f32,
    resize_offset: Option<f32>,
}
impl TableColumn {
    fn new(kind: ColumnKind) -> Self {
        let width = match kind {
            ColumnKind::UpdateTime => 130.0,
            ColumnKind::Symbol => 80.0,
            ColumnKind::OrderType => 50.0,
            ColumnKind::Side => 50.0,
            ColumnKind::Price => 100.0,
            ColumnKind::OrigQty => 80.0,
            ColumnKind::ExecutedQty => 80.0,
            ColumnKind::ReduceOnly => 100.0,
            ColumnKind::TimeInForce => 50.0,
            ColumnKind::CancelOrder => 60.0,
        };

        Self {
            kind,
            width,
            resize_offset: None,
        }
    }
}
enum ColumnKind {
    Symbol,
    Side,
    Price,
    OrigQty,
    ExecutedQty,
    TimeInForce,
    OrderType,
    ReduceOnly,
    UpdateTime,
    CancelOrder
}
struct TableRow {
    order: user_data::NewOrder,
}
impl TableRow {
    fn add_row(order: user_data::NewOrder) -> Self {
        Self {
            order,
        }
    }
    fn update_row(&mut self, order: user_data::NewOrder) {
        self.order = order;
    }
    fn remove_row(order_id: i64, rows: &mut Vec<TableRow>) {
        if let Some(index) = rows.iter().position(|r| r.order.order_id == order_id) {
            rows.remove(index);
        }
    }
}
impl<'a> table::Column<'a, Message, Theme, Renderer> for TableColumn {
    type Row = TableRow;

    fn header(&'a self, _col_index: usize) -> Element<'a, Message> {
        let content = match self.kind {
            ColumnKind::UpdateTime => "Time",
            ColumnKind::Symbol => "Symbol",
            ColumnKind::OrderType => "Type",
            ColumnKind::Side => "Side",
            ColumnKind::Price => "Price",
            ColumnKind::OrigQty => "Amount",
            ColumnKind::ExecutedQty => "Filled",
            ColumnKind::ReduceOnly => "Reduce Only",
            ColumnKind::TimeInForce => "TIF",
            ColumnKind::CancelOrder => "Cancel",
        };

        container(text(content)).height(24).center_y().into()
    }

    fn cell(
        &'a self,
        _col_index: usize,
        row_index: usize,
        row: &'a Self::Row,
    ) -> Element<'a, Message> {
        let content: Element<_> = match self.kind {
            ColumnKind::UpdateTime => text(row.order.update_time.to_string()).into(),
            ColumnKind::Symbol => text(&row.order.symbol).into(),
            ColumnKind::OrderType => text(&row.order.order_type).into(),
            ColumnKind::Side => text(&row.order.side).into(),
            ColumnKind::Price => text(&row.order.price).into(),
            ColumnKind::OrigQty => text(&row.order.orig_qty).into(),
            ColumnKind::ExecutedQty => text(&row.order.executed_qty).into(),
            ColumnKind::ReduceOnly => text(row.order.reduce_only.to_string()).into(),
            ColumnKind::TimeInForce => text(&row.order.time_in_force).into(),
            ColumnKind::CancelOrder => button("X").on_press(Message::CancelOrder(row.order.order_id.to_string())).into(),
        };

        container(content)
            .width(Length::Fill)
            .height(32)
            .center_y()
            .into()
    }

    fn width(&self) -> f32 {
        self.width
    }

    fn resize_offset(&self) -> Option<f32> {
        self.resize_offset
    }
}

struct PosTableColumn {
    kind: PosColumnKind,
    width: f32,
    resize_offset: Option<f32>,
}
impl PosTableColumn {
    fn new(kind: PosColumnKind) -> Self {
        let width = match kind {
            PosColumnKind::Symbol => 100.0,
            PosColumnKind::PosSize => 100.0,
            PosColumnKind::EntryPrice => 100.0,
            PosColumnKind::Breakeven => 100.0,
            PosColumnKind::MarkPrice => 100.0,
            PosColumnKind::LiqPrice => 100.0,
            PosColumnKind::MarginAmt => 100.0,
            PosColumnKind::UnrealPnL => 100.0,
        };

        Self {
            kind,
            width,
            resize_offset: None,
        }
    }
}
enum PosColumnKind {
    Symbol,
    PosSize,
    EntryPrice,
    Breakeven,
    MarkPrice,
    LiqPrice,
    MarginAmt,
    UnrealPnL,
}
#[derive(Debug, Clone)]
struct PosTableRow {
    position: user_data::PositionInTable,
}
impl PosTableRow {
    fn add_row(position: user_data::PositionInTable) -> Self {
        Self {
            position,
        }
    }
    fn remove_row(symbol: &String, rows: &mut Vec<PosTableRow>) {
        if let Some(index) = rows.iter().position(|r| r.position.symbol == *symbol) {
            rows.remove(index);
        }
    }
}
impl<'a> table::Column<'a, Message, Theme, Renderer> for PosTableColumn {
    type Row = PosTableRow;

    fn header(&'a self, _col_index: usize) -> Element<'a, Message> {
        let content = match self.kind {
            PosColumnKind::Symbol => "Symbol",
            PosColumnKind::PosSize => "Size",
            PosColumnKind::EntryPrice => "Entry",
            PosColumnKind::Breakeven => "Breakeven",
            PosColumnKind::MarkPrice => "Mark Price",
            PosColumnKind::LiqPrice => "Liq Price",
            PosColumnKind::MarginAmt => "Margin",
            PosColumnKind::UnrealPnL => "PnL",
        };

        container(text(content)).height(24).center_y().into()
    }

    fn cell(
        &'a self,
        _col_index: usize,
        row_index: usize,
        row: &'a Self::Row,
    ) -> Element<'a, Message> {
        let content: Element<_> = match self.kind {
            PosColumnKind::Symbol => text(row.position.symbol.to_string()).into(),
            PosColumnKind::PosSize => text(&row.position.size).into(),
            PosColumnKind::EntryPrice => text(&row.position.entry_price).into(),
            PosColumnKind::Breakeven => text(&row.position.breakeven_price).into(),
            PosColumnKind::MarkPrice => text(&row.position.mark_price).into(),
            PosColumnKind::LiqPrice => text(&row.position.liquidation_price).into(),
            PosColumnKind::MarginAmt => text(&row.position.margin_amt).into(),
            PosColumnKind::UnrealPnL => text(&row.position.unrealized_pnl).into(),
        };

        container(content)
            .width(Length::Fill)
            .height(32)
            .center_y()
            .into()
    }

    fn width(&self) -> f32 {
        self.width
    }

    fn resize_offset(&self) -> Option<f32> {
        self.resize_offset
    }
}