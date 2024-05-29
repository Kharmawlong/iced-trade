use std::collections::{BTreeMap, HashMap, VecDeque};
use chrono::{DateTime, Utc, TimeZone, LocalResult, Duration, NaiveDateTime, Timelike};
use iced::{
    alignment, color, mouse, widget::{button, canvas::{self, event::{self, Event}, path, stroke::Stroke, Cache, Canvas, Geometry, Path}}, window, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector
};
use iced::widget::{Column, Row, Container, Text};
use crate::data_providers::binance::market_data::Trade;

#[derive(Debug, Clone)]
pub enum Message {
    Translated(Vector),
    Scaled(f32, Option<Vector>),
    ChartBounds(Rectangle),
    AutoscaleToggle,
    CrosshairToggle,
    CrosshairMoved(Point),
}

#[derive(Debug)]
pub struct Heatmap {
    heatmap_cache: Cache,
    crosshair_cache: Cache,
    x_labels_cache: Cache,
    y_labels_cache: Cache,
    y_croshair_cache: Cache,
    x_crosshair_cache: Cache,
    translation: Vector,
    scaling: f32,
    
    data_points: VecDeque<(DateTime<Utc>, f32, f32, bool)>,
    depth: VecDeque<(DateTime<Utc>, Vec<(f32, f32)>, Vec<(f32, f32)>)>,
    size_filter: f32,

    autoscale: bool,
    crosshair: bool,
    crosshair_position: Point,
    x_min_time: i64,
    x_max_time: i64,
    y_min_price: f32,
    y_max_price: f32,
    bounds: Rectangle,

    timeframe: f32,
}
impl Heatmap {
    const MIN_SCALING: f32 = 0.4;
    const MAX_SCALING: f32 = 3.6;

    pub fn new() -> Heatmap {
        let _size = window::Settings::default().size;
    
        Heatmap {
            heatmap_cache: canvas::Cache::default(),
            crosshair_cache: canvas::Cache::default(),
            x_labels_cache: canvas::Cache::default(),
            y_labels_cache: canvas::Cache::default(),
            y_croshair_cache: canvas::Cache::default(),
            x_crosshair_cache: canvas::Cache::default(),

            data_points: VecDeque::new(),
            depth: VecDeque::new(),
            size_filter: 0.0,

            translation: Vector::default(),
            scaling: 1.0,
            autoscale: true,
            crosshair: false,
            crosshair_position: Point::new(0.0, 0.0),
            x_min_time: 0,
            x_max_time: 0,
            y_min_price: 0.0,
            y_max_price: 0.0,
            bounds: Rectangle::default(),
            timeframe: 0.5,
        }
    }

    pub fn set_size_filter(&mut self, size_filter: f32) {
        self.size_filter = size_filter;
    }

    pub fn insert_datapoint(&mut self, mut trades_buffer: Vec<Trade>, depth_update: u64, bids: Vec<(f32, f32)>, asks: Vec<(f32, f32)>) {
        let aggregate_time = 100; 
        let seconds = (depth_update / 1000) as i64;
        let nanoseconds = ((depth_update % 1000) / aggregate_time * aggregate_time * 1_000_000) as u32;
        let depth_update_time: DateTime<Utc> = match Utc.timestamp_opt(seconds, nanoseconds) {
            LocalResult::Single(dt) => dt,
            _ => return, 
        };

        for trade in trades_buffer.drain(..) {
            self.data_points.push_back((depth_update_time, trade.price, trade.qty, trade.is_sell));
        }
        if let Some((time, _, _)) = self.depth.back() {
            if *time == depth_update_time {
                self.depth.pop_back();
            }
        }
        self.depth.push_back((depth_update_time, bids, asks));

        while self.data_points.len() > 6000 {
            self.data_points.pop_front();
        }
        while self.depth.len() > 1000 {
            self.depth.pop_front();
        }

        self.render_start();
    }
    
    pub fn render_start(&mut self) {
        let timestamp_now = Utc::now().timestamp_millis();

        let latest: i64 = timestamp_now - ((self.translation.x*100.0)*(self.timeframe as f32)) as i64;
        let earliest: i64 = latest - ((64000.0*self.timeframe as f32) / (self.scaling / (self.bounds.width/800.0))) as i64;

        let visible_trades: Vec<&(DateTime<Utc>, f32, f32, bool)> = self.data_points.iter()
            .filter(|(time, _, _, _)| {
                let timestamp = time.timestamp_millis();
                timestamp >= earliest && timestamp <= latest
            })
            .collect::<Vec<_>>();

        let visible_depth = self.depth.iter()
            .filter(|(time, _, _)| {
                let timestamp = time.timestamp_millis();
                timestamp >= earliest && timestamp <= latest
            })
            .collect::<Vec<_>>();

        if visible_trades.is_empty() || visible_trades.len() < 5 {
            return;
        }

        let highest = visible_depth.iter().map(|(_, bids, asks)| {
            let highest_bid = bids.iter().map(|(price, _)| price).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);
            let highest_ask = asks.iter().map(|(price, _)| price).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);

            highest_bid.max(*highest_ask)
        }).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(0.0);
        let lowest = visible_depth.iter().map(|(_, bids, asks)| {
            let lowest_bid = bids.iter().map(|(price, _)| price).min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);
            let lowest_ask = asks.iter().map(|(price, _)| price).min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);

            lowest_bid.min(*lowest_ask)
        }).min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(0.0);

        if earliest != self.x_min_time || latest != self.x_max_time {            
            self.x_labels_cache.clear();
            self.x_crosshair_cache.clear();
        }
        if lowest != self.y_min_price || highest != self.y_max_price {            
            self.y_labels_cache.clear();
            self.y_croshair_cache.clear();
        }

        self.x_min_time = earliest;
        self.x_max_time = latest;
        self.y_min_price = lowest;
        self.y_max_price = highest;
        
        self.crosshair_cache.clear();

        self.heatmap_cache.clear();
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Translated(translation) => {
                if self.autoscale {
                    self.translation.x = translation.x;
                } else {
                    self.translation = translation;
                }
                self.crosshair_position = Point::new(0.0, 0.0);

                self.render_start();
            }
            Message::Scaled(scaling, translation) => {
                self.scaling = scaling;
                
                if let Some(translation) = translation {
                    if self.autoscale {
                        self.translation.x = translation.x;
                    } else {
                        self.translation = translation;
                    }
                }
                self.crosshair_position = Point::new(0.0, 0.0);

                self.render_start();
            }
            Message::ChartBounds(bounds) => {
                self.bounds = bounds;
            }
            Message::AutoscaleToggle => {
                self.autoscale = !self.autoscale;
            }
            Message::CrosshairToggle => {
                self.crosshair = !self.crosshair;
            }
            Message::CrosshairMoved(position) => {
                self.crosshair_position = position;
                if self.crosshair {
                    self.crosshair_cache.clear();
                    self.y_croshair_cache.clear();
                    self.x_crosshair_cache.clear();
                }
            }
        }
    }

    pub fn view(&self) -> Element<Message> {
        let chart = Canvas::new(self)
            .width(Length::FillPortion(10))
            .height(Length::FillPortion(10));
        
        let axis_labels_x = Canvas::new(
            AxisLabelXCanvas { 
                labels_cache: &self.x_labels_cache, 
                min: self.x_min_time, 
                max: self.x_max_time, 
                crosshair_cache: &self.x_crosshair_cache, 
                crosshair_position: self.crosshair_position, 
                crosshair: self.crosshair,
            })
            .width(Length::FillPortion(10))
            .height(Length::Fixed(26.0));

        let axis_labels_y = Canvas::new(
            AxisLabelYCanvas { 
                labels_cache: &self.y_labels_cache, 
                y_croshair_cache: &self.y_croshair_cache, 
                min: self.y_min_price,
                max: self.y_max_price,
                crosshair_position: self.crosshair_position, 
                crosshair: self.crosshair
            })
            .width(Length::Fixed(60.0))
            .height(Length::FillPortion(10));

        let autoscale_button = button(
            Text::new("A")
                .size(12)
                .horizontal_alignment(alignment::Horizontal::Center)
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Message::AutoscaleToggle)
            .style(|_theme: &Theme, _status: iced::widget::button::Status| chart_button(_theme, &_status, self.autoscale));
        let crosshair_button = button(
            Text::new("+")
                .size(12)
                .horizontal_alignment(alignment::Horizontal::Center)
            ) 
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Message::CrosshairToggle)
            .style(|_theme: &Theme, _status: iced::widget::button::Status| chart_button(_theme, &_status, self.crosshair));
    
        let chart_controls = Container::new(
            Row::new()
                .push(autoscale_button)
                .push(crosshair_button).spacing(2)
            ).padding([0, 2, 0, 2])
            .width(Length::Fixed(60.0))
            .height(Length::Fixed(26.0));

        let chart_and_y_labels = Row::new()
            .push(chart)
            .push(axis_labels_y);
    
        let bottom_row = Row::new()
            .push(axis_labels_x)
            .push(chart_controls);
    
        let content = Column::new()
            .push(chart_and_y_labels)
            .push(bottom_row)
            .spacing(0)
            .padding(5);
    
        content.into()
    }
}

fn chart_button(_theme: &Theme, _status: &button::Status, is_active: bool) -> button::Style {
    button::Style {
        background: Some(Color::from_rgba8(20, 20, 20, 1.0).into()),
        border: Border {
            color: {
                if is_active {
                    Color::from_rgba8(50, 50, 50, 1.0)
                } else {
                    Color::from_rgba8(20, 20, 20, 1.0)
                }
            },
            width: 1.0,
            radius: 2.0.into(),
        },
        text_color: Color::WHITE,
        ..button::Style::default()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Interaction {
    None,
    Drawing,
    Erasing,
    Panning { translation: Vector, start: Point },
}

impl Default for Interaction {
    fn default() -> Self {
        Self::None
    }
}
impl canvas::Program<Message> for Heatmap {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Interaction,
        event: Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {        
        if bounds != self.bounds {
            return (event::Status::Ignored, Some(Message::ChartBounds(bounds)));
        } 
        
        if let Event::Mouse(mouse::Event::ButtonReleased(_)) = event {
            *interaction = Interaction::None;
        }

        let Some(cursor_position) = cursor.position_in(bounds) else {
            return (event::Status::Ignored, 
                if self.crosshair {
                    Some(Message::CrosshairMoved(Point::new(0.0, 0.0)))
                } else {
                    None
                }
                );
        };

        match event {
            Event::Mouse(mouse_event) => match mouse_event {
                mouse::Event::ButtonPressed(button) => {
                    let message = match button {
                        mouse::Button::Right => {
                            *interaction = Interaction::Drawing;
                            None
                        }
                        mouse::Button::Left => {
                            *interaction = Interaction::Panning {
                                translation: self.translation,
                                start: cursor_position,
                            };
                            None
                        }
                        _ => None,
                    };

                    (event::Status::Captured, message)
                }
                mouse::Event::CursorMoved { .. } => {
                    let message = match *interaction {
                        Interaction::Drawing => None,
                        Interaction::Erasing => None,
                        Interaction::Panning { translation, start } => {
                            Some(Message::Translated(
                                translation
                                    + (cursor_position - start)
                                        * (1.0 / self.scaling),
                            ))
                        }
                        Interaction::None => 
                            if self.crosshair && cursor.is_over(bounds) {
                                Some(Message::CrosshairMoved(cursor_position))
                            } else {
                                None
                            },
                    };

                    let event_status = match interaction {
                        Interaction::None => event::Status::Ignored,
                        _ => event::Status::Captured,
                    };

                    (event_status, message)
                }
                mouse::Event::WheelScrolled { delta } => match delta {
                    mouse::ScrollDelta::Lines { y, .. }
                    | mouse::ScrollDelta::Pixels { y, .. } => {
                        if y < 0.0 && self.scaling > Self::MIN_SCALING
                            || y > 0.0 && self.scaling < Self::MAX_SCALING
                        {
                            //let old_scaling = self.scaling;

                            let scaling = (self.scaling * (1.0 + y / 30.0))
                                .clamp(
                                    Self::MIN_SCALING,  // 0.1
                                    Self::MAX_SCALING,  // 2.0
                                );

                            //let translation =
                            //    if let Some(cursor_to_center) =
                            //        cursor.position_from(bounds.center())
                            //    {
                            //        let factor = scaling - old_scaling;

                            //        Some(
                            //            self.translation
                            //                - Vector::new(
                            //                    cursor_to_center.x * factor
                            //                        / (old_scaling
                            //                            * old_scaling),
                            //                    cursor_to_center.y * factor
                            //                        / (old_scaling
                            //                            * old_scaling),
                            //                ),
                            //        )
                            //    } else {
                            //        None
                            //    };

                            (
                                event::Status::Captured,
                                Some(Message::Scaled(scaling, None)),
                            )
                        } else {
                            (event::Status::Captured, None)
                        }
                    }
                },
                _ => (event::Status::Ignored, None),
            },
            _ => (event::Status::Ignored, None),
        }
    }
    
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {    
        let latest = self.x_max_time;
        let earliest = self.x_min_time;
    
        let (filtered_visible_trades, visible_trades) = self.data_points.iter()
            .filter(|(time, _, _, _)| {
                let timestamp = time.timestamp_millis();
                timestamp >= earliest && timestamp <= latest
            })
            .fold((vec![], vec![]), |(mut filtered, mut visible), trade| {
                visible.push(*trade);
                if (trade.2 * trade.1) >= self.size_filter {
                    filtered.push(*trade);
                }
                (filtered, visible)
            });

        let visible_depth: Vec<&(DateTime<Utc>, Vec<(f32, f32)>, Vec<(f32, f32)>)> = self.depth.iter()
            .filter(|(time, _, _)| {
                let timestamp = time.timestamp_millis();
                timestamp >= earliest && timestamp <= latest
            })
            .collect::<Vec<_>>();

        let highest = self.y_max_price;
        let lowest = self.y_min_price;

        let y_range = highest - lowest;

        let volume_area_height = bounds.height / 8.0; 
        let heatmap_area_height = bounds.height - volume_area_height;

        let heatmap = self.heatmap_cache.draw(renderer, bounds.size(), |frame| {
            // volume bars
            let mut aggregated_volumes: HashMap<i64, (f32, f32)> = HashMap::new();
            for &(time, _, qty, is_sell) in &visible_trades {
                let timestamp = time.timestamp_millis();
                aggregated_volumes.entry(timestamp).and_modify(|e| {
                    if is_sell {
                        e.1 += qty;
                    } else {
                        e.0 += qty;
                    }
                }).or_insert(if is_sell { (0.0, qty) } else { (qty, 0.0) });
            }

            let max_volume = aggregated_volumes.iter().map(|(_, (buy, sell))| buy.max(*sell)).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(0.0);
            for (&timestamp, &(buy_volume, sell_volume)) in &aggregated_volumes {
                let x_position = ((timestamp - earliest) as f64 / (latest - earliest) as f64) * bounds.width as f64;

                let buy_bar_height = (buy_volume / max_volume) * volume_area_height;
                let sell_bar_height = (sell_volume / max_volume) * volume_area_height;

                let sell_bar = Path::rectangle(
                    Point::new(x_position as f32, (bounds.height - sell_bar_height) as f32), 
                    Size::new(1.0, sell_bar_height as f32)
                );
                frame.fill(&sell_bar, Color::from_rgb8(192, 80, 77)); 

                let buy_bar = Path::rectangle(
                    Point::new(x_position as f32 + 2.0, (bounds.height - buy_bar_height) as f32), 
                    Size::new(1.0, buy_bar_height as f32)
                );
                frame.fill(&buy_bar, Color::from_rgb8(81, 205, 160)); 
            }

            // trades
            if filtered_visible_trades.len() > 1 {
                let (qty_max, qty_min) = filtered_visible_trades.iter().map(|(_, _, qty, _)| qty).fold((0.0f32, f32::MAX), |(max, min), &qty| (max.max(qty), min.min(qty)));
                for &(time, price, qty, is_sell) in &filtered_visible_trades {
                    let timestamp = time.timestamp_millis();
                    let x_position = ((timestamp - earliest) as f64 / (latest - earliest) as f64) * bounds.width as f64;
                    let y_position = heatmap_area_height - ((price - lowest) / y_range * heatmap_area_height);

                    let color = if is_sell {
                        Color::from_rgba8(192, 80, 77, 1.0)
                    } else {
                        Color::from_rgba8(81, 205, 160, 1.0)
                    };

                    let radius = 1.0 + (qty - qty_min) * (35.0 - 1.0) / (qty_max - qty_min);

                    let circle = Path::circle(Point::new(x_position as f32, y_position), radius);
                    frame.fill(&circle, color);
                }
            }
            
            // orderbook heatmap
            let max_order_quantity = visible_depth.iter()
                .map(|(_, bids, asks)| {
                bids.iter().map(|(_, qty)| qty).chain(asks.iter().map(|(_, qty)| qty)).fold(f32::MIN, |current_max: f32, qty: &f32| f32::max(current_max, *qty))
            }).fold(f32::MIN, f32::max);
            for i in 0..20 { 
                let bids_i: Vec<(DateTime<Utc>, f32, f32)> = visible_depth.iter()
                    .map(|&(time, bid, _ask)| ((*time).clone(), bid[i].0, bid[i].1)).collect();
                let asks_i: Vec<(DateTime<Utc>, f32, f32)> = visible_depth.iter()
                    .map(|&(time, _bid, ask)| ((*time).clone(), ask[i].0, ask[i].1)).collect();

                bids_i.iter().zip(asks_i.iter()).for_each(|((time, bid_price, bid_qty), (_, ask_price, ask_qty))| {
                    let bid_y_position = heatmap_area_height - ((bid_price - lowest) / y_range * heatmap_area_height);
                    let ask_y_position = heatmap_area_height - ((ask_price - lowest) / y_range * heatmap_area_height);

                    let x_position = ((time.timestamp_millis() - earliest) as f64 / (latest - earliest) as f64) * bounds.width as f64;

                    let bid_color_alpha = (bid_qty / max_order_quantity).min(1.0);
                    let ask_color_alpha = (ask_qty / max_order_quantity).min(1.0);

                    let bid_circle = Path::circle(Point::new(x_position as f32, bid_y_position), 1.0);
                    frame.fill(&bid_circle, Color::from_rgba8(0, 144, 144, bid_color_alpha));


                    let ask_circle = Path::circle(Point::new(x_position as f32, ask_y_position), 1.0);
                    frame.fill(&ask_circle, Color::from_rgba8(192, 0, 192, ask_color_alpha));
                });
            }
        });

        if self.crosshair {
            let crosshair = self.crosshair_cache.draw(renderer, bounds.size(), |frame| {
                if let Some(cursor_position) = cursor.position_in(bounds) {
                    let line = Path::line(
                        Point::new(0.0, cursor_position.y), 
                        Point::new(bounds.width as f32, cursor_position.y)
                    );
                    frame.stroke(&line, Stroke::default().with_color(Color::from_rgba8(200, 200, 200, 0.6)).with_width(1.0));

                    let crosshair_ratio = cursor_position.x as f64 / bounds.width as f64;
                    let crosshair_millis = (earliest as f64 + crosshair_ratio * (latest as f64 - earliest as f64)).round() / 100.0 * 100.0;
                    let crosshair_time = NaiveDateTime::from_timestamp((crosshair_millis / 1000.0).floor() as i64, ((crosshair_millis % 1000.0) * 1_000_000.0).round() as u32);

                    let crosshair_timestamp = crosshair_time.timestamp_millis() as i64;

                    let snap_ratio = (crosshair_timestamp as f64 - earliest as f64) / ((latest as f64) - (earliest as f64));
                    let snap_x = snap_ratio * bounds.width as f64;

                    let line = Path::line(
                        Point::new(snap_x as f32, 0.0), 
                        Point::new(snap_x as f32, bounds.height as f32)
                    );
                    frame.stroke(&line, Stroke::default().with_color(Color::from_rgba8(200, 200, 200, 0.6)).with_width(1.0));
                }
            });

            return vec![crosshair, heatmap];
        }   else {
            return vec![heatmap];
        }
    }

    fn mouse_interaction(
        &self,
        interaction: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match interaction {
            Interaction::Drawing => mouse::Interaction::Crosshair,
            Interaction::Erasing => mouse::Interaction::Crosshair,
            Interaction::Panning { .. } => mouse::Interaction::Grabbing,
            Interaction::None if cursor.is_over(bounds) => {
                if self.crosshair {
                    mouse::Interaction::Crosshair
                } else {
                    mouse::Interaction::default()
                }
            }
            Interaction::None => { mouse::Interaction::default() }
        }
    }
}

fn calculate_price_step(highest: f32, lowest: f32, labels_can_fit: i32) -> (f32, f32) {
    let range = highest - lowest;
    let mut step = 1000.0; 

    let steps = [1000.0, 500.0, 200.0, 100.0, 50.0, 20.0, 10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05];

    for &s in steps.iter().rev() {
        if range / s <= labels_can_fit as f32 {
            step = s;
            break;
        }
    }
    let rounded_lowest = (lowest / step).floor() * step;

    (step, rounded_lowest)
}
fn calculate_time_step(earliest: i64, latest: i64, labels_can_fit: i32) -> (Duration, NaiveDateTime) {
    let duration = latest - earliest;

    let steps = [
        Duration::minutes(1),
        Duration::seconds(30),
        Duration::seconds(15),
        Duration::seconds(10),
        Duration::seconds(5),
        Duration::seconds(2),
        Duration::seconds(1),
        Duration::milliseconds(500),
    ];

    let mut selected_step = steps[0];
    for &step in steps.iter() {
        if duration / step.num_milliseconds() >= labels_can_fit as i64 {
            selected_step = step;
            break;
        }
    }

    let rounded_earliest = NaiveDateTime::from_timestamp(
        (earliest / 1000) / (selected_step.num_milliseconds() / 1000) * (selected_step.num_milliseconds() / 1000),
        0
    );

    (selected_step, rounded_earliest)
}

pub struct AxisLabelXCanvas<'a> {
    labels_cache: &'a Cache,
    crosshair_cache: &'a Cache,
    crosshair_position: Point,
    crosshair: bool,
    min: i64,
    max: i64,
}
impl canvas::Program<Message> for AxisLabelXCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        _interaction: &mut Interaction,
        _event: Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {
        (event::Status::Ignored, None)
    }
    
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if self.max == 0 {
            return vec![];
        }
        let latest_in_millis = self.max; 
        let earliest_in_millis = self.min; 

        let x_labels_can_fit = (bounds.width / 120.0) as i32;
        let (time_step, rounded_earliest) = calculate_time_step(self.min, self.max, x_labels_can_fit);

        let labels = self.labels_cache.draw(renderer, bounds.size(), |frame| {
            frame.with_save(|frame| {
                let mut time = rounded_earliest;
                let latest_time = NaiveDateTime::from_timestamp(latest_in_millis / 1000, 0);

                while time <= latest_time {
                    let time_in_millis = time.timestamp_millis();
                    
                    let x_position = ((time_in_millis - earliest_in_millis) as f64 / (latest_in_millis - earliest_in_millis) as f64) * bounds.width as f64;

                    if x_position >= 0.0 && x_position <= bounds.width as f64 {
                        let text_size = 12.0;
                        let label = canvas::Text {
                            content: time.format("%M:%S").to_string(),
                            position: Point::new(x_position as f32 - text_size, bounds.height as f32 - 20.0),
                            size: iced::Pixels(text_size),
                            color: Color::from_rgba8(200, 200, 200, 1.0),
                            ..canvas::Text::default()
                        };  

                        label.draw_with(|path, color| {
                            frame.fill(&path, color);
                        });
                    }
                    
                    time = time + time_step;
                }
            });
        });
        let crosshair = self.crosshair_cache.draw(renderer, bounds.size(), |frame| {
            if self.crosshair && self.crosshair_position.x > 0.0 {
                let crosshair_ratio = self.crosshair_position.x as f64 / bounds.width as f64;
                let crosshair_millis = (earliest_in_millis as f64 + crosshair_ratio * (latest_in_millis as f64 - earliest_in_millis as f64)).round() / 100.0 * 100.0;
                let crosshair_time = NaiveDateTime::from_timestamp((crosshair_millis / 1000.0).floor() as i64, ((crosshair_millis % 1000.0) * 1_000_000.0).round() as u32);
                
                let crosshair_timestamp = crosshair_time.timestamp_millis() as i64;
                let time = NaiveDateTime::from_timestamp(crosshair_timestamp / 1000, 0);

                let snap_ratio = (crosshair_timestamp as f64 - earliest_in_millis as f64) / (latest_in_millis as f64 - earliest_in_millis as f64);
                let snap_x = snap_ratio * bounds.width as f64;

                let text_size = 12.0;
                let text_content = time.format("%M:%S").to_string();
                let growth_amount = 6.0; 
                let rectangle_position = Point::new(snap_x as f32 - 14.0 - growth_amount, bounds.height as f32 - 20.0);
                let text_position = Point::new(snap_x as f32 - 14.0, bounds.height as f32 - 20.0);

                let text_background = canvas::Path::rectangle(rectangle_position, Size::new(text_content.len() as f32 * text_size/2.0 + 2.0 * growth_amount + 1.0, text_size + text_size/2.0));
                frame.fill(&text_background, Color::from_rgba8(200, 200, 200, 1.0));

                let crosshair_label = canvas::Text {
                    content: text_content,
                    position: text_position,
                    size: iced::Pixels(text_size),
                    color: Color::from_rgba8(0, 0, 0, 1.0),
                    ..canvas::Text::default()
                };

                crosshair_label.draw_with(|path, color| {
                    frame.fill(&path, color);
                });
            }
        });

        vec![labels, crosshair]
    }

    fn mouse_interaction(
        &self,
        interaction: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match interaction {
            Interaction::Drawing => mouse::Interaction::Crosshair,
            Interaction::Erasing => mouse::Interaction::Crosshair,
            Interaction::Panning { .. } => mouse::Interaction::ResizingHorizontally,
            Interaction::None if cursor.is_over(bounds) => {
                mouse::Interaction::ResizingHorizontally
            }
            Interaction::None => mouse::Interaction::default(),
        }
    }
}

pub struct AxisLabelYCanvas<'a> {
    labels_cache: &'a Cache,
    y_croshair_cache: &'a Cache,
    min: f32,
    max: f32,
    crosshair_position: Point,
    crosshair: bool,
}
impl canvas::Program<Message> for AxisLabelYCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        _interaction: &mut Interaction,
        _event: Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {
        (event::Status::Ignored, None)
    }
    
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if self.max == 0.0 {
            return vec![];
        }

        let y_labels_can_fit = (bounds.height / 32.0) as i32;
        let (step, rounded_lowest) = calculate_price_step(self.max, self.min, y_labels_can_fit);

        let volume_area_height = bounds.height / 8.0; 
        let candlesticks_area_height = bounds.height - volume_area_height;

        let labels = self.labels_cache.draw(renderer, bounds.size(), |frame| {
            frame.with_save(|frame| {
                let y_range = self.max - self.min;
                let mut y = rounded_lowest;

                while y <= self.max {
                    let y_position = candlesticks_area_height - ((y - self.min) / y_range * candlesticks_area_height);

                    let text_size = 12.0;
                    let decimal_places = if step.fract() == 0.0 { 0 } else { 1 };
                    let label_content = match decimal_places {
                        0 => format!("{:.0}", y),
                        _ => format!("{:.1}", y),
                    };
                    let label = canvas::Text {
                        content: label_content,
                        position: Point::new(10.0, y_position - text_size / 2.0),
                        size: iced::Pixels(text_size),
                        color: Color::from_rgba8(200, 200, 200, 1.0),
                        ..canvas::Text::default()
                    };  

                    label.draw_with(|path, color| {
                        frame.fill(&path, color);
                    });

                    y += step;
                }
            });
        });
        let crosshair = self.y_croshair_cache.draw(renderer, bounds.size(), |frame| {
            if self.crosshair && self.crosshair_position.y > 0.0 {
                let text_size = 12.0;
                let y_range = self.max - self.min;
                let label_content = format!("{:.1}", self.min + (y_range * (candlesticks_area_height - self.crosshair_position.y) / candlesticks_area_height));
                
                let growth_amount = 3.0; 
                let rectangle_position = Point::new(8.0 - growth_amount, self.crosshair_position.y - text_size / 2.0 - 3.0);
                let text_position = Point::new(8.0, self.crosshair_position.y - text_size / 2.0 - 3.0);

                let text_background = canvas::Path::rectangle(rectangle_position, Size::new(label_content.len() as f32 * text_size / 2.0 + 2.0 * growth_amount + 4.0, text_size + text_size / 1.8));
                frame.fill(&text_background, Color::from_rgba8(200, 200, 200, 1.0));

                let label = canvas::Text {
                    content: label_content,
                    position: text_position,
                    size: iced::Pixels(text_size),
                    color: Color::from_rgba8(0, 0, 0, 1.0),
                    ..canvas::Text::default()
                };

                label.draw_with(|path, color| {
                    frame.fill(&path, color);
                });
            }
        });

        vec![labels, crosshair]
    }

    fn mouse_interaction(
        &self,
        interaction: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match interaction {
            Interaction::Drawing => mouse::Interaction::Crosshair,
            Interaction::Erasing => mouse::Interaction::Crosshair,
            Interaction::Panning { .. } => mouse::Interaction::ResizingVertically,
            Interaction::None if cursor.is_over(bounds) => {
                mouse::Interaction::ResizingVertically
            }
            Interaction::None => mouse::Interaction::default(),
        }
    }
}