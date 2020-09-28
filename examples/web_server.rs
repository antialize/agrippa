use agrippa::tcp::listen;
use agrippa::{
    runtime::{Priority, Reactor, Result},
    tcp::ListenSocket,
    tcp::Socket,
    util::spawn_task,
};
use log::info;

extern crate simple_logger;

async fn handle_client(socket: &mut Socket) -> Result<()> {
    let mut data = [0; 100];
    let len = socket.read(&mut data).await?;
    info!("Read {}", len);
    Ok(())
}

async fn accept_connections(listener: &mut ListenSocket) -> Result<()> {
    loop {
        let mut socket = listener.accept().await?;

        spawn_task(Priority::Normal, async move {
            let res = handle_client(&mut socket).await;
            info!("Closing client connection");
            socket.close().await?;
            res?;
            info!("Connection closed");
            Ok(())
        })
        .await?;
    }
}

fn main() -> Result<()> {
    simple_logger::init().unwrap();

    let reactor = Reactor::new(1024 * 1024)?;

    let r2 = reactor.clone();
    Reactor::spawn(&reactor.clone(), Priority::Normal, async move {
        let mut listener = listen("127.0.0.1:1234").await?;
        let ret = accept_connections(&mut listener).await;
        listener.close().await?;
        return ret;
    });

    r2.run()?;

    Ok(())
}
