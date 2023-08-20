//! 内容源自rich_text的快照，可滚动的浏览的组件。

use std::cell::RefCell;
use std::collections::{HashMap};
use std::ops::Deref;
use std::rc::Rc;
use fltk::draw::{draw_rect_fill, Offscreen};
use fltk::enums::{Align, Color, Cursor, Event};
use fltk::frame::Frame;
use fltk::group::{Scroll, ScrollType};
use fltk::prelude::{GroupExt, WidgetBase, WidgetExt};
use fltk::{app, draw, widget_extends};
use crate::{Coordinates, LinedData, LinePiece, PADDING, RichData, UserData};

#[derive(Clone, Debug)]
pub struct RichReviewer {
    pub(crate) scroller: Scroll,
    pub(crate) panel: Frame,
    data_buffer: Rc<RefCell<Vec<RichData>>>,
    background_color: Rc<RefCell<Color>>,
    reviewer_screen: Rc<RefCell<Offscreen>>,
    visible_lines: Rc<RefCell<HashMap<Coordinates, usize>>>,
}
widget_extends!(RichReviewer, Scroll, scroller);

impl RichReviewer {
    pub const SCROLL_BAR_WIDTH: i32 = 10;
    pub const PANEL_MAX_HEIGHT: i32 = 10;

    pub fn new<T>(x: i32, y: i32, w: i32, h: i32, title: T) -> Self
        where T: Into<Option<&'static str>> + Clone {
        let mut scroller = Scroll::new(x, y, w, h, title);
        scroller.set_type(ScrollType::Vertical);
        scroller.set_scrollbar_size(Self::SCROLL_BAR_WIDTH);
        scroller.set_align(Align::Bottom);
        scroller.end();

        let mut panel = Frame::new(x, y, w, h, None);
        scroller.add_resizable(&panel);
        // scroller.resizable(&panel);
        // scroller.set_clip_children(true);

        let data_buffer: Rc<RefCell<Vec<RichData>>> = Rc::new(RefCell::new(vec![]));
        let background_color = Rc::new(RefCell::new(Color::Black));
        let visible_lines = Rc::new(RefCell::new(HashMap::<Coordinates, usize>::new()));
        let notifier: Rc<RefCell<Option<tokio::sync::mpsc::Sender<UserData>>>> = Rc::new(RefCell::new(None));
        let reviewer_screen = Rc::new(RefCell::new(Offscreen::new(w, h).unwrap()));

        /*
        利用tokio::spawn的协程特性，在app主线程中异步执行滚动操作。
        在非主线程的其他独立线程中，是无法执行滚动操作的。
         */
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<i32>(10);
        let mut scroll_rc = scroller.clone();
        tokio::spawn(async move {
            while let Some(y) = receiver.recv().await {
                scroll_rc.scroll_to(0, y);
            }
        });

        panel.draw({
            let data_buffer_rc = data_buffer.clone();
            let scroll_rc = scroller.clone();
            let visible_lines_rc = visible_lines.clone();
            let bg_rc = background_color.clone();
            let screen_rc = reviewer_screen.clone();
            move |ctx| {
                Self::draw_offline(screen_rc.clone(), &scroll_rc, data_buffer_rc.clone(), bg_rc.borrow().clone(), visible_lines_rc.clone());

                // let (x, y, window_width, window_height) = (ctx.x(), ctx.y(), ctx.width(), scroll_rc.height());
                let (x, y, window_width, window_height) = (scroll_rc.x(), scroll_rc.y(), scroll_rc.width(), scroll_rc.height());
                screen_rc.borrow().copy(x, y, window_width, window_height, 0, 0);
            }
        });

        scroller.handle({
            let buffer_rc = data_buffer.clone();
            let bg_color_rc = background_color.clone();
            let last_window_size = Rc::new(RefCell::new((0, 0)));
            let visible_lines_rc = visible_lines.clone();
            let notifier_rc = notifier.clone();
            let screen_rc = reviewer_screen.clone();
            let mut panel_rc = panel.clone();
            move |scroller, evt| {
                match evt {
                    Event::Resize => {
                        // 缩放窗口后重新计算分片绘制信息。
                        let (current_width, current_height) = (scroller.width(), scroller.height());
                        let last_panel_height = panel_rc.height();
                        let (last_width, last_height) = *last_window_size.borrow();
                        if last_width != current_width || last_height != current_height {
                            last_window_size.replace((current_width, current_height));

                            let mut new_panel_height = current_height;
                            if last_width != current_width {
                                // 当窗口宽度发生变化时，需要重新计算数据分片坐标信息。
                                let drawable_max_width = current_width - PADDING.left - PADDING.right;
                                let mut last_piece = LinePiece::init_piece();
                                for rich_data in buffer_rc.borrow_mut().iter_mut() {
                                    rich_data.line_pieces.clear();
                                    last_piece = rich_data.estimate(last_piece, drawable_max_width);
                                }

                                new_panel_height = Self::calc_panel_height(buffer_rc.clone(), current_height);
                                panel_rc.resize(scroller.x(), scroller.y(), current_width, new_panel_height);
                            }

                            // 按照新的窗口大小重新生成绘图板
                            if let Some(offs) = Offscreen::new(current_width, current_height) {
                                screen_rc.replace(offs);
                            } else {
                                println!("Failed to create Offscreen.");
                            }

                            let old_scroll_y = scroller.yposition();
                            println!("更新绘图板，old_scroll_y {old_scroll_y}");
                            if old_scroll_y > 0 && last_height > 0 {
                                let pos_percent = old_scroll_y as f64 / (last_panel_height - last_height) as f64;
                                println!("pos_percent: {}", pos_percent);
                                let new_scroll_y = ((new_panel_height - current_height) as f64 * pos_percent).round() as i32;
                                println!("new scroll y: {}", new_scroll_y);
                                // scroller.scroll_to(0, new_scroll_y);
                                let sender = sender.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = sender.send(new_scroll_y).await {
                                        println!("发送滚动信号失败！{:?}", e);
                                    }
                                });
                            }
                        }
                    }
                    Event::Move => {
                        // 检测鼠标进入可互动区域，改变鼠标样式
                        let mut enter_piece = false;
                        for area in visible_lines_rc.borrow().keys() {
                            let (x, y, w, h) = area.to_rect();
                            if app::event_inside(x, y, w, h) {
                                enter_piece = true;
                                break;
                            }
                        }
                        if enter_piece {
                            draw::set_cursor(Cursor::Hand);
                        } else {
                            draw::set_cursor(Cursor::Default);
                        }
                    }
                    Event::Released => {
                        // 检测鼠标点击可互动区域，执行用户自定义操作
                        for (area, idx) in visible_lines_rc.borrow().iter() {
                            let (x, y, w, h) = area.to_rect();
                            if app::event_inside(x, y, w, h) {
                                if let Some(rd) = buffer_rc.borrow().get(*idx) {
                                    let sd = rd.into();
                                    if let Some(notifier) = notifier_rc.borrow().as_ref() {
                                        let notifier = notifier.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = notifier.send(sd).await {
                                                eprintln!("send error: {:?}", e);
                                            }
                                        });
                                    }
                                }
                                break;
                            }
                        }
                    }
                    _ => {}
                }
                false
            }
        });


        Self { scroller, panel, data_buffer, background_color, reviewer_screen, visible_lines }
    }

    pub fn set_background_color(&self, color: Color) {
        self.background_color.replace(color);
    }

    pub fn set_data(&mut self, data: Vec<RichData>) {
        // 更新回看数据
        self.data_buffer.replace(data);

        let (scroller_width, scroller_height) = (self.panel.width(), self.scroller.height());

        // 设置新的窗口尺寸
        let panel_height = Self::calc_panel_height(self.data_buffer.clone(), scroller_height);
        self.panel.resize(self.panel.x(), self.panel.y(), scroller_width, panel_height);
    }

    /// 计算数据内容所需的面板高度。
    ///
    /// # Arguments
    ///
    /// * `buffer`:
    /// * `scroller_height`:
    ///
    /// returns: i32
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    pub fn calc_panel_height(buffer_rc: Rc<RefCell<Vec<RichData>>>, scroller_height: i32) -> i32 {
        let buffer = &*buffer_rc.borrow();
        let (mut top, mut bottom) = (0, 0);
        if let Some(first) = buffer.first() {
            if let Some((top_y, _, _)) = first.v_bounds {
                top = top_y;
            }
        }
        if let Some(last) = buffer.last() {
            if let Some((_, bottom_y, _)) = last.v_bounds {
                bottom = bottom_y;
            }
        }
        let mut content_height = bottom - top + PADDING.bottom + PADDING.top;
        if content_height > scroller_height {
            content_height
        } else {
            scroller_height
        }
    }

    pub fn draw_offline(screen: Rc<RefCell<Offscreen>>, scroller: &Scroll, data_buffer: Rc<RefCell<Vec<RichData>>>, background_color: Color, visible_lines: Rc<RefCell<HashMap<Coordinates, usize>>>) {
        screen.borrow().begin();
        // 滚动条滚动的高度在0到(panel.height - scroll.height)之间。
        let mut base_y = scroller.yposition();
        if base_y < 0 {
            base_y = 0;
        }

        let window_width = scroller.width();
        let window_height = scroller.height();
        let (mut top_y, mut bottom_y) = (base_y, base_y + window_height);

        // 处理数据相对位移
        if let Some(first) = data_buffer.borrow().first() {
            if let Some((y, _, _)) = first.v_bounds {
                top_y += y;
                bottom_y += y;
            }
        }

        let offset_y = top_y - PADDING.top;

        // 填充背景色
        draw_rect_fill(0, 0, window_width, window_height, background_color);

        let mut data = &*data_buffer.borrow();

        /*
        先试算出可显示的行，再真正绘制可显示的行。
        试算从数据队列的尾部向头部取数，试算位置从窗口底部向顶部堆积。
         */
        let (mut from_index, mut to_index, total_len) = (0, data.len(), data.len());
        let mut set_to_index = false;
        let mut begin_check_from_index = false;
        for (seq, rich_data) in data.iter().rev().enumerate() {
            if !set_to_index && rich_data.is_visible(top_y, bottom_y) {
                // 待绘制的内容超出窗口底部边界
                to_index = total_len - seq;
                set_to_index = true;
                begin_check_from_index = true;
            }

            if begin_check_from_index && !rich_data.is_visible(top_y, bottom_y) {
                // 待绘制内容已经向上超出窗口顶部边界，可以停止处理前面的数据了。
                from_index = total_len - seq;
                break;
            }
        }

        for (idx, rich_data) in data[from_index..to_index].iter().enumerate() {
            rich_data.draw(offset_y, idx, visible_lines.clone());
        }

        screen.borrow().end();
    }

    /// 根据当前回顾`scroller`窗口大小创建对应的离线绘图板，并设置滚动条到最底部。
    ///
    /// # Arguments
    ///
    /// * `w`:
    /// * `h`:
    ///
    /// returns: ()
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    pub fn renew_offscreen(&mut self, w: i32, h: i32) {
        if let Some(offs) = Offscreen::new(w, h) {
            self.reviewer_screen.replace(offs);
            // 滚动到最底部
            self.scroller.scroll_to(0, self.panel.height() - self.scroller.height());
        }
    }
}