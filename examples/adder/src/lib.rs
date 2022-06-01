pub mod ext {
    use serde::{Deserialize, Serialize};

    include!(concat!(env!("OUT_DIR"), "/adder-metadata.rs"));
}

#[cfg(test)]
mod tests {
    use crate::ext::ExtContract;
    use test_log::test;
    use workspaces::network::DevAccountDeployer;

    #[test(tokio::test)]
    async fn it_works() -> anyhow::Result<()> {
        let worker = workspaces::sandbox().await?;
        let contract = worker
            .dev_deploy(include_bytes!("../res/adder.wasm"))
            .await?;

        let contract = ExtContract { contract: contract };
        let res = contract.add(&worker, vec![3, 4], vec![2, 1]).await?;
        assert_eq!(res, [5, 5]);

        Ok(())
    }
}
