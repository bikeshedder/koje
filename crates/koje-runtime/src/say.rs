//! Where the runtime's messages go: to stdout when the CLI runs it, to the
//! caller of the operation when kojed does (`OperationProgress`, ADR 0060).
//! The sink is per thread: kojed runs each operation on one blocking thread.

use std::cell::RefCell;

type Sink = Box<dyn Fn(&str)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// One line for the user.
pub fn line(text: &str) {
    SINK.with(|s| match &*s.borrow() {
        Some(sink) => sink(text),
        None => println!("{text}"),
    });
}

/// Run `f` with its lines going to `sink` instead of stdout.
pub fn to<T>(sink: impl Fn(&str) + 'static, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<Sink>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let before = self.0.take();
            SINK.with(|s| *s.borrow_mut() = before);
        }
    }
    let before = SINK.with(|s| s.borrow_mut().replace(Box::new(sink)));
    let _reset = Reset(before);
    f()
}

/// `say!("…", …)`: [`line`] with `format!` arguments.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {{
        $crate::say::line(&format!($($arg)*));
    }};
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn lines_go_to_the_sink_while_it_is_set() {
        let got = Rc::new(RefCell::new(Vec::new()));
        let g = Rc::clone(&got);
        let n = to(
            move |l| g.borrow_mut().push(l.to_owned()),
            || {
                say!("one {}", 1);
                to(|_| {}, || say!("swallowed"));
                say!("two");
                7
            },
        );
        assert_eq!(n, 7);
        assert_eq!(*got.borrow(), ["one 1", "two"]);
        SINK.with(|s| assert!(s.borrow().is_none()));
    }
}
