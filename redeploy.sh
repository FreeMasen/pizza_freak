PID=`getpid pizza_freak`
cargo build --release && kill -kill $PID && RUST_LOG=pizza_freak:info,pizza_freak:error ./target/release/pizza_freak > ~/logs/pizza_freak.log &