//! Icons drawn with cairo in the widget's own text color, so they never go
//! missing ("!") whatever icon theme is installed, and follow light/dark,
//! checked tiles, etc. 24x24 design grid, scaled to any size.

use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{cairo, gdk};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Glyph {
    /// 0 = off, 1..=3 = signal strength
    Wifi(u8),
    Bluetooth,
    /// percent, charging
    Battery(u8, bool),
    Rotate,
    Settings,
    Lock,
    Moon,
    Restart,
    Power,
    Logout,
    Sun,
    Volume,
    Mic,
}

#[derive(Clone)]
pub struct Icon {
    pub area: gtk::DrawingArea,
    glyph: Rc<Cell<Glyph>>,
}

impl Icon {
    pub fn new(g: Glyph, px: i32) -> Icon {
        let area = gtk::DrawingArea::new();
        area.set_content_width(px);
        area.set_content_height(px);
        area.set_valign(gtk::Align::Center);
        let glyph = Rc::new(Cell::new(g));
        let gl = glyph.clone();
        area.set_draw_func(move |a, cr, w, h| {
            let color = a.color();
            draw(cr, gl.get(), f64::from(w), f64::from(h), color);
        });
        Icon { area, glyph }
    }

    pub fn set(&self, g: Glyph) {
        if self.glyph.get() != g {
            self.glyph.set(g);
            self.area.queue_draw();
        }
    }
}

fn rrect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn draw(cr: &cairo::Context, g: Glyph, w: f64, h: f64, c: gdk::RGBA) {
    let s = w.min(h) / 24.0;
    cr.translate((w - 24.0 * s) / 2.0, (h - 24.0 * s) / 2.0);
    cr.scale(s, s);
    let (r, gr, b, a) = (
        f64::from(c.red()),
        f64::from(c.green()),
        f64::from(c.blue()),
        f64::from(c.alpha()),
    );
    let set = |alpha: f64| cr.set_source_rgba(r, gr, b, a * alpha);
    set(1.0);
    cr.set_line_width(2.0);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    match g {
        Glyph::Wifi(level) => {
            cr.set_line_width(2.4);
            for (i, rad) in [5.0, 9.5, 14.0].iter().enumerate() {
                set(if usize::from(level) > i { 1.0 } else { 0.3 });
                cr.new_sub_path();
                cr.arc(12.0, 19.5, *rad, 1.25 * PI, 1.75 * PI);
                let _ = cr.stroke();
            }
            set(if level > 0 { 1.0 } else { 0.3 });
            cr.arc(12.0, 19.5, 1.8, 0.0, 2.0 * PI);
            let _ = cr.fill();
            if level == 0 {
                set(1.0);
                cr.move_to(4.0, 4.0);
                cr.line_to(20.0, 20.0);
                let _ = cr.stroke();
            }
        }
        Glyph::Bluetooth => {
            cr.move_to(7.0, 7.5);
            cr.line_to(17.0, 16.0);
            cr.line_to(12.0, 20.5);
            cr.line_to(12.0, 3.5);
            cr.line_to(17.0, 8.0);
            cr.line_to(7.0, 16.5);
            let _ = cr.stroke();
        }
        Glyph::Battery(pct, charging) => {
            cr.push_group();
            cr.set_line_width(1.8);
            rrect(cr, 2.5, 7.0, 17.0, 10.0, 2.5);
            let _ = cr.stroke();
            rrect(cr, 20.0, 10.0, 2.0, 4.0, 0.8);
            let _ = cr.fill();
            let fill_w = 13.0 * f64::from(pct.min(100)) / 100.0;
            if pct <= 15 && !charging {
                cr.set_source_rgba(0.95, 0.35, 0.3, a);
            } else {
                set(if charging { 0.55 } else { 1.0 });
            }
            cr.rectangle(4.5, 9.0, fill_w, 6.0);
            let _ = cr.fill();
            if charging {
                // lightning bolt with a transparent halo around it
                let bolt = |cr: &cairo::Context| {
                    cr.move_to(13.0, 4.0);
                    cr.line_to(8.0, 12.8);
                    cr.line_to(11.3, 12.8);
                    cr.line_to(10.0, 20.0);
                    cr.line_to(15.5, 10.8);
                    cr.line_to(12.0, 10.8);
                    cr.close_path();
                };
                cr.set_operator(cairo::Operator::Clear);
                bolt(cr);
                cr.set_line_width(2.6);
                let _ = cr.stroke();
                cr.set_operator(cairo::Operator::Over);
                set(1.0);
                bolt(cr);
                let _ = cr.fill();
            }
            let _ = cr.pop_group_to_source();
            let _ = cr.paint();
        }
        Glyph::Rotate => {
            let _ = cr.save();
            cr.translate(12.0, 12.0);
            cr.rotate(PI / 4.0);
            rrect(cr, -3.5, -7.5, 7.0, 15.0, 1.8);
            let _ = cr.stroke();
            let _ = cr.restore();
            cr.new_sub_path();
            cr.arc(12.0, 12.0, 10.0, 1.05 * PI, 1.35 * PI);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(12.0, 12.0, 10.0, 0.05 * PI, 0.35 * PI);
            let _ = cr.stroke();
        }
        Glyph::Settings => {
            cr.push_group();
            for i in 0..8 {
                let _ = cr.save();
                cr.translate(12.0, 12.0);
                cr.rotate(f64::from(i) * PI / 4.0);
                cr.rectangle(-1.7, -10.5, 3.4, 4.5);
                let _ = cr.restore();
            }
            let _ = cr.fill();
            cr.arc(12.0, 12.0, 7.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.set_operator(cairo::Operator::Clear);
            cr.arc(12.0, 12.0, 2.8, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.set_operator(cairo::Operator::Over);
            let _ = cr.pop_group_to_source();
            let _ = cr.paint();
        }
        Glyph::Lock => {
            cr.move_to(7.8, 12.0);
            cr.line_to(7.8, 10.0);
            cr.arc(12.0, 10.0, 4.2, PI, 2.0 * PI);
            cr.line_to(16.2, 12.0);
            let _ = cr.stroke();
            rrect(cr, 5.5, 11.0, 13.0, 9.5, 2.0);
            let _ = cr.fill();
        }
        Glyph::Moon => {
            cr.push_group();
            cr.arc(11.0, 12.5, 8.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.set_operator(cairo::Operator::Clear);
            cr.arc(15.5, 8.5, 7.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.set_operator(cairo::Operator::Over);
            let _ = cr.pop_group_to_source();
            let _ = cr.paint();
        }
        Glyph::Restart => {
            cr.new_sub_path();
            cr.arc(12.0, 12.5, 7.5, -0.2 * PI, 1.45 * PI);
            let _ = cr.stroke();
            cr.move_to(13.5, 3.2);
            cr.line_to(19.8, 3.6);
            cr.line_to(18.2, 9.6);
            cr.close_path();
            let _ = cr.fill();
        }
        Glyph::Power => {
            cr.new_sub_path();
            cr.arc(12.0, 13.0, 7.5, -0.5 * PI + 0.75, 1.5 * PI - 0.75);
            let _ = cr.stroke();
            cr.move_to(12.0, 3.0);
            cr.line_to(12.0, 11.0);
            let _ = cr.stroke();
        }
        Glyph::Logout => {
            cr.move_to(12.0, 4.5);
            cr.line_to(5.0, 4.5);
            cr.line_to(5.0, 19.5);
            cr.line_to(12.0, 19.5);
            cr.move_to(10.0, 12.0);
            cr.line_to(20.0, 12.0);
            cr.move_to(16.5, 8.5);
            cr.line_to(20.0, 12.0);
            cr.line_to(16.5, 15.5);
            let _ = cr.stroke();
        }
        Glyph::Sun => {
            cr.arc(12.0, 12.0, 4.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
            for i in 0..8 {
                let t = f64::from(i) * PI / 4.0;
                cr.move_to(12.0 + 7.0 * t.cos(), 12.0 + 7.0 * t.sin());
                cr.line_to(12.0 + 9.5 * t.cos(), 12.0 + 9.5 * t.sin());
            }
            let _ = cr.stroke();
        }
        Glyph::Mic => {
            rrect(cr, 9.0, 3.0, 6.0, 11.0, 3.0);
            let _ = cr.fill();
            cr.new_sub_path();
            cr.arc(12.0, 10.5, 6.0, 0.0, PI);
            cr.move_to(12.0, 16.5);
            cr.line_to(12.0, 20.5);
            cr.move_to(8.5, 20.5);
            cr.line_to(15.5, 20.5);
            let _ = cr.stroke();
        }
        Glyph::Volume => {
            cr.move_to(3.5, 9.0);
            cr.line_to(7.5, 9.0);
            cr.line_to(12.5, 4.5);
            cr.line_to(12.5, 19.5);
            cr.line_to(7.5, 15.0);
            cr.line_to(3.5, 15.0);
            cr.close_path();
            let _ = cr.fill();
            cr.new_sub_path();
            cr.arc(13.0, 12.0, 4.0, -0.3 * PI, 0.3 * PI);
            let _ = cr.stroke();
            cr.new_sub_path();
            cr.arc(13.0, 12.0, 7.5, -0.3 * PI, 0.3 * PI);
            let _ = cr.stroke();
        }
    }
}
