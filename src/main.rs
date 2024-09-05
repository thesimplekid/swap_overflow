use std::time::Duration;

use anyhow::bail;
use cdk::amount::SplitTarget;
use cdk::dhke::construct_proofs;
use cdk::nuts::{CurrencyUnit, Id, KeySet, MintQuoteState, PreMintSecrets, Proofs, SwapRequest};
use cdk::Amount;
use cdk::HttpClient;
use tokio::time::sleep;

const MINT_URL: &str = "https://mint.thesimplekid.dev";

async fn mint_ecash(
    wallet_client: HttpClient,
    keyset_id: Id,
    mint_keys: KeySet,
) -> anyhow::Result<Proofs> {
    println!("Minting for ecash");
    println!();
    let mint_quote = wallet_client
        .post_mint_quote(MINT_URL.parse()?, 1.into(), CurrencyUnit::Sat)
        .await?;

    println!("Please pay: {}", mint_quote.request);

    loop {
        let status = wallet_client
            .get_mint_quote_status(MINT_URL.parse()?, &mint_quote.quote)
            .await?;

        if status.state == MintQuoteState::Paid {
            break;
        }
        println!("{:?}", status.state);

        sleep(Duration::from_secs(2)).await;
    }

    let premint_secrets =
        PreMintSecrets::random(keyset_id.clone(), 1.into(), &SplitTarget::default())?;

    let mint_response = wallet_client
        .post_mint(
            MINT_URL.parse()?,
            &mint_quote.quote,
            premint_secrets.clone(),
        )
        .await?;

    let pre_swap_proofs = construct_proofs(
        mint_response.signatures,
        premint_secrets.rs(),
        premint_secrets.secrets(),
        &mint_keys.clone().keys,
    )?;

    println!(
        "Pre swap amount: {:?}",
        pre_swap_proofs.iter().map(|p| p.amount).sum::<Amount>()
    );

    println!(
        "Pre swap amounts: {:?}",
        pre_swap_proofs
            .iter()
            .map(|p| p.amount)
            .collect::<Vec<Amount>>()
    );

    Ok(pre_swap_proofs)
}

async fn swap_with_overflow(
    wallet_client: HttpClient,
    keyset_id: Id,
    mint_keys: KeySet,
    pre_swap_proofs: Proofs,
) -> anyhow::Result<Proofs> {
    println!();
    println!("Attempting to swap with amounts that will overflow");
    println!(
        "Using Inputs: {:?}",
        pre_swap_proofs
            .iter()
            .map(|p| p.amount)
            .collect::<Vec<Amount>>()
    );
    // Construct messages that will overflow

    let amount = 2_u64.pow(63);

    let pre_mint_amount =
        PreMintSecrets::random(keyset_id.clone(), amount.into(), &SplitTarget::default())?;
    let pre_mint_amount_two =
        PreMintSecrets::random(keyset_id.clone(), amount.into(), &SplitTarget::default())?;

    let mut pre_mint =
        PreMintSecrets::random(keyset_id.clone(), 1.into(), &SplitTarget::default())?;

    pre_mint.combine(pre_mint_amount);
    pre_mint.combine(pre_mint_amount_two);

    let swap_request = SwapRequest::new(pre_swap_proofs.clone(), pre_mint.blinded_messages());

    let swap_response = match wallet_client
        .post_swap(MINT_URL.parse()?, swap_request)
        .await
    {
        Ok(res) => res,
        Err(_err) => bail!("Request error on swap"),
    };

    let post_swap_proofs = construct_proofs(
        swap_response.signatures,
        pre_mint.rs(),
        pre_mint.secrets(),
        &mint_keys.clone().keys,
    )?;

    println!(
        "Post swap amount: {:?}",
        post_swap_proofs.iter().map(|p| p.amount).sum::<Amount>()
    );

    println!(
        "Post swap amounts: {:?}",
        post_swap_proofs
            .iter()
            .map(|p| p.amount)
            .collect::<Vec<Amount>>()
    );

    Ok(post_swap_proofs)
}

async fn swap_with_ecash_created_by_overflow(
    wallet_client: HttpClient,
    keyset_id: Id,
    mint_keys: KeySet,
    pre_swap_proofs: Proofs,
) -> anyhow::Result<Proofs> {
    println!();
    println!("Attempting another swap with ecash minted by overflow.");
    println!(
        "Using Inputs: {:?}",
        pre_swap_proofs
            .iter()
            .map(|p| p.amount)
            .collect::<Vec<Amount>>()
    );

    let amount = pre_swap_proofs.iter().map(|p| p.amount).sum();

    let pre_second_swap =
        PreMintSecrets::random(keyset_id.clone(), amount, &SplitTarget::default())?;
    let swap_request =
        SwapRequest::new(pre_swap_proofs.clone(), pre_second_swap.blinded_messages());

    let swap_response = match wallet_client
        .post_swap(MINT_URL.parse()?, swap_request)
        .await
    {
        Ok(res) => res,
        Err(_err) => bail!("Could not swap"),
    };

    let post_swap_proofs = construct_proofs(
        swap_response.signatures,
        pre_second_swap.rs(),
        pre_second_swap.secrets(),
        &mint_keys.clone().keys,
    )?;

    println!(
        "Post swap amounts: {:?}",
        post_swap_proofs
            .iter()
            .map(|p| p.amount)
            .collect::<Vec<Amount>>()
    );

    Ok(post_swap_proofs)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let wallet_client = HttpClient::new();

    let keysets = wallet_client.get_mint_keysets(MINT_URL.parse()?).await?;

    let active: Vec<&cdk::nuts::KeySetInfo> = keysets.keysets.iter().filter(|k| k.active).collect();
    let keyset_id = active.first().unwrap().id;

    let mint_keys = wallet_client
        .get_mint_keyset(MINT_URL.parse()?, keyset_id)
        .await?;

    let pre_swap_proofs = mint_ecash(wallet_client.clone(), keyset_id, mint_keys.clone()).await?;

    let post_swap_proofs = swap_with_overflow(
        wallet_client.clone(),
        keyset_id,
        mint_keys.clone(),
        pre_swap_proofs.clone(),
    )
    .await?;

    let _ = swap_with_ecash_created_by_overflow(
        wallet_client,
        keyset_id,
        mint_keys,
        post_swap_proofs[..2].to_vec(),
    )
    .await?;

    bail!("Should not have been able to swap")
}
