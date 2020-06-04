use agrippa::runtime::{Priority, Reactor, Result};
use agrippa::tcp::connect;
use agrippa::verbs;
use agrippa::verbs_util;

use log::info;

extern crate simple_logger;

fn main() -> Result<()> {
    simple_logger::init().unwrap();

    let reactor = Reactor::new(1024 * 1024)?;
    let r2 = reactor.clone();

    Reactor::spawn(&reactor.clone(), Priority::Normal, async move {
        let mut socket = connect("127.0.0.1:1234").await?;
        println!("CONNECTED");

        let connection_builder = verbs::connect().await?;
        let addr = connection_builder.local_address();
        info!("Send addr {:?}", addr);
        socket.write_item(&addr).await;
        let remote_addr = unsafe { socket.read_item::<verbs::VerbsAddr>().await? };
        socket.close().await?;
        info!("Got remote addr {:?}", remote_addr);
        let conn = connection_builder.connect(&remote_addr)?;

        let buffer = verbs::get_buffer().await?;
        info!("Filling buffer");
        //TODO FILL IN BUFFER
        conn.send(buffer).await?;

        info!("SENT EVERYTHING");

        Ok(())
    });

    r2.run()?;

    Ok(())
}
