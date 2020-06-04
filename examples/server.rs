use agrippa::runtime::{Priority, Reactor, Result};
use agrippa::tcp::listen;
use agrippa::verbs;
use agrippa::verbs_util;
use log::info;

extern crate simple_logger;

fn main() -> Result<()> {
    simple_logger::init().unwrap();

    let reactor = Reactor::new(1024 * 1024)?;

    let r2 = reactor.clone();
    Reactor::spawn(&reactor.clone(), Priority::Normal, async move {
        let listener = listen("127.0.0.1:1234").await?;
        loop {
            info!("XX");
            let mut socket = listener.accept().await?;
            info!("KQQ");
            let r3 = reactor.clone();
            Reactor::spawn(&reactor.clone(), Priority::Normal, async move {
                info!("Got connection from");

                //socket.write("hi there\n".as_bytes()).await?;

                //let mut buffer = [0 as u8; 10

                let remote_addr = unsafe { socket.read_item::<verbs_util::VerbsAddr>().await? };

                let connection_builder = verbs::connect().await?;
                let local_addr = connection_builder.local_address();

                info!("Got addr {:?}", remote_addr);
                //let read = socket.read(&mut buffer).await?;

                socket.write_item(&local_addr).await?;
                info!("Send addr {:?}", local_addr);

                let conn = connection_builder.connect(&remote_addr)?;

                info!("Connected");

                let buff = conn.recv().await?;
                info!("Got message");

                verbs::put_buffer(buff).await?;

                //r3.device.connect(&addr)?;
                //info!("Y");
                //r3.device.recv_ping2().await?;
                //info!("X");

                /*info!(
                    "Got some {}\n",
                    std::str::from_utf8(&buffer[..read]).expect("MONKEY")
                );*/

                info!("CLOSED");
                socket.close().await?;
                Ok(())
            });
        }
        //listener.close().await?;
        //println!("CLOSED");
        Ok(())
    });

    r2.run()?;

    Ok(())
}
