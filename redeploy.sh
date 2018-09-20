PID=`getpid pizza_freak`
cargo build --release
kill -kill $PID
./target/release/pizza_freak