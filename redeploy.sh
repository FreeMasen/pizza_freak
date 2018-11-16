PID=`getpid pizza_freak`
cargo build #--release
if [ $? -ne 0 ]; then
    exit $?
fi
if [ -z "$PID" ]; then
    echo no current pizza_freak running
else
    echo killing old pizza_freak at $PID
    kill -kill $PID
fi
./start.sh
