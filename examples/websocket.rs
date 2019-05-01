use {
    futures::{Async, Future, Poll, Stream},
    http::Response,
    izanami::{ws::Message, Context, Handler, Launcher},
};

#[derive(Default)]
struct MyHandler(());

impl Handler for MyHandler {
    type Body = izanami::body::Body;
    type Error = izanami::error::Error;

    fn poll_http(&mut self, cx: &mut Context<'_>) -> Poll<Response<Self::Body>, Self::Error> {
        if let Some((res, stream)) = cx.start_websocket()? {
            tokio::spawn({
                let (tx, rx) = stream.split();
                rx.filter_map(move |m| match m {
                    Message::Ping(p) => Some(Message::Pong(p)),
                    Message::Pong(_) => None,
                    _ => Some(m),
                }) //
                .forward(tx)
                .then(move |_| Ok(()))
            });
            Ok(Async::Ready(res.map(Into::into)))
        } else {
            Ok(Async::Ready(
                Response::builder()
                    .header("content-type", "text/html")
                    .body(CLIENT.into())
                    .unwrap(),
            ))
        }
    }
}

fn main() -> failure::Fallible<()> {
    let mut launcher = Launcher::new(MyHandler::default)?;
    launcher.bind("127.0.0.1:4000")?;
    launcher.run_forever();
    Ok(())
}

const CLIENT: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8" />
    <title>WebSocket Example</title>
    <style>
        #log {
            width: 20em;
            height: 15em;
            overflow: auto;
            border: 1px solid black;
        }
    </style>
</head>
<body>
    <h3>Connection</h3>
    <div>
        Status: <span id="status">disconnected</span>
        <button id="connect">Connect</button>
    </div>

    <h3>Form</h3>
    <form onsubmit="return false;">
        <input id="text" type="text" />
        <input id="send" type="button" value="Send" />
    </form>

    <h3>Log</h3>
    <div id="log"></div>
</body>

<script type="text/javascript">
    const log = (msg) => {
        let elem = document.getElementById("log");
        elem.innerHTML += msg + '<br />';
        elem.scrollTop += 1000;
    };

    let conn = null;

    const connect = () => {
        disconnect();
        conn = new WebSocket('ws://' + window.location.host + '/ws');
        log('Connecting...');

        conn.onopen = function() {
            log('Connected.');
            update_ui();
        };

        conn.onmessage = function(e) {
            log('Received: ' + e.data);
        };

        conn.onclose = function() {
            log('Disconnected.');
            conn = null;
            update_ui();
        };
    };

    const disconnect = () => {
        if (conn == null) { return }

        log('Disconnecting...');
        conn.close();
        conn = null;
        update_ui();
    };

    const update_ui = () => {
        if (conn == null) {
            document.getElementById('status').innerText = 'disconnected';
            document.getElementById('connect').innerHTML = 'Connect';
        } else {
            document.getElementById('status').innerText = 'connected';
            document.getElementById('connect').innerHTML = 'Disconnect';
        }
    }

    document.getElementById('connect').onclick = () => {
        if (conn == null) {
            connect();
        } else {
            disconnect();
        }
        update_ui();
        return false;
    };

    document.getElementById('send').onclick = () => {
        let text_node = document.getElementById('text');
        let text = text_node.value;
        log('Sending: ' + text);
        conn.send(text);
        text_node.value = '';
        text_node.focus();
        return false;
    };

    document.getElementById('text').onkeyup = (e) => {
        if (e.keyCode === 13) {
            document.getElementById('send').click();
            return false;
        }
    };
</script>
</html>
"#;
