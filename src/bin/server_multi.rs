use std::{env, net::SocketAddr, sync::Arc};

use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream, tcp::WriteHalf},
    select,
    sync::broadcast::{self, Receiver, Sender},
};

type ClientID = u16;

#[derive(Clone, Debug)]
struct BroadcastMessage {
    origin: ClientID,
    message: String,
}

struct Connection {
    client_addr: SocketAddr,
    rx: Receiver<BroadcastMessage>,
    tx: Arc<Sender<BroadcastMessage>>,
}

async fn listen(port: u16) -> io::Result<()> {
    let socket = TcpSocket::new_v4()?;
    let socket_addr = SocketAddr::new("127.0.0.1".parse::<_>().unwrap(), port);
    socket.set_reuseaddr(true)?;
    socket.bind(socket_addr)?;
    let listener = socket.listen(1024)?;
    println!("listening on port {}", socket_addr.port());

    let (tx, _rx): (Sender<BroadcastMessage>, Receiver<BroadcastMessage>) =
        broadcast::channel::<BroadcastMessage>(16);
    let broadcast_tx = Arc::new(tx);
    loop {
        let connection = listener.accept().await;
        if let Err(err) = connection {
            eprintln!("server: {}", err);
            continue;
        }
        let (socket, peer) = connection.unwrap();
        let new_rx = broadcast_tx.subscribe();
        let new_tx = broadcast_tx.clone();
        tokio::spawn(async move {
            println!("connected {} {}", peer.ip(), peer.port());
            let conn = Connection {
                client_addr: peer,
                rx: new_rx,
                tx: new_tx,
            };
            process(socket, conn).await;
        });
    }
}

async fn send_message_ack(write: &mut WriteHalf<'_>) -> io::Result<()> {
    write.write(b"ACK:MESSAGE\n").await?;
    Ok(())
}

async fn send_broadcast(write: &mut WriteHalf<'_>, msg: BroadcastMessage) -> io::Result<()> {
    write
        .write(format!("MESSAGE:{} {}", msg.origin, msg.message).as_bytes())
        .await?;
    Ok(())
}

async fn process(mut stream: TcpStream, mut conn: Connection) {
    let login_msg = format!("LOGIN:{}\n", conn.client_addr.port());
    let _ = stream.write(login_msg.as_bytes()).await;
    let (read, mut write) = stream.split();
    let mut reader = tokio::io::BufReader::new(read);

    let mut buf = String::new();
    loop {
        let read_future = reader.read_line(&mut buf);
        let broadcast_future = conn.rx.recv();

        select! {
            _ = read_future => {
                let message = BroadcastMessage {
                    origin: conn.client_addr.port(),
                    message: buf.clone() // todo: buf does not escape
                };
                print!("message {} {}", message.origin, message.message);
                conn.tx.send(message).expect("failed write");
                send_message_ack(&mut write).await.expect("failed ack");
            },
            broadcast_result = broadcast_future => {
                let msg = broadcast_result.expect("invalid broadcast");
                if msg.origin != conn.client_addr.port() {
                    send_broadcast(&mut write, msg).await.expect("failed write");
                };
            },
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let port: u16 = args
        .get(1)
        .expect("Missing port number")
        .parse()
        .expect("Invalid port number");
    listen(port).await.unwrap();
}
