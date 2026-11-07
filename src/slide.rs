//! A one-child container that draws its child shifted vertically by `offset`
//! pixels. Moving it only changes a transform (no relayout), so dragging and
//! animating sheets stays smooth even with big app grids inside.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{graphene, gsk};

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct Slide {
        pub offset: Cell<f64>,
        pub child: RefCell<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Slide {
        const NAME: &'static str = "RustTouchSlide";
        type Type = super::Slide;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Slide {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Slide {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match self.child.borrow().as_ref() {
                Some(c) => c.measure(orientation, for_size),
                None => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(c) = self.child.borrow().as_ref() {
                let t = gsk::Transform::new()
                    .translate(&graphene::Point::new(0.0, self.offset.get() as f32));
                c.allocate(width, height, baseline, Some(t));
            }
        }
    }
}

glib::wrapper! {
    pub struct Slide(ObjectSubclass<imp::Slide>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Slide {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let s: Self = glib::Object::new();
        child.set_parent(&s);
        *s.imp().child.borrow_mut() = Some(child.clone().upcast());
        s
    }

    pub fn set_offset(&self, y: f64) {
        self.imp().offset.set(y);
        self.queue_allocate();
    }

    pub fn offset(&self) -> f64 {
        self.imp().offset.get()
    }
}
