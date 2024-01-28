use env_logger::TimestampPrecision;
use figment::{providers::Env, Error, Figment};
use futures_util::StreamExt;
use jito_protos::{
    convert::versioned_tx_from_packet,
    searcher::{
        mempool_subscription, searcher_service_client::SearcherServiceClient, MempoolSubscription,
        NextScheduledLeaderRequest, PendingTxNotification, ProgramSubscriptionV0,
    }
};
use jito_searcher_client::{get_searcher_client, token_authenticator::ClientInterceptor};
use log::info;
use serde::Deserialize;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, transaction::VersionedTransaction};
use solana_transaction_status::EncodableWithMeta;
use spl_token::instruction::TokenInstruction;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use tonic::{codegen::InterceptedService, transport::Channel, Streaming};

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub keypair_path: String,
    pub block_engine_url: String,
    pub token_program_account: String,
}

impl Config {
    pub fn load() -> Result<Config, Error> {
        let config: Config = Figment::new().merge(Env::prefixed("APP_")).extract()?;
        Ok(config)
    }
}

#[tokio::main]
async fn main() {
    // Loading enviroment
    dotenv::dotenv().ok();
    let config = Config::load().unwrap();

    // Configuring logger
    env_logger::builder()
        .format_timestamp(Some(TimestampPrecision::Micros))
        .init();

    // Initializing Jito client
    let keypair = Arc::new(read_keypair_file(config.keypair_path).expect("reads keypair at path"));
    let mut client = get_searcher_client(&config.block_engine_url, &keypair)
        .await
        .expect("connects to searcher client");

    // Subscribing to mempool
    info!(
        "Listening for transactions on token program {}",
        config.token_program_account
    );
    let pending_transactions = client
        .subscribe_mempool(MempoolSubscription {
            msg: Some(mempool_subscription::Msg::ProgramV0Sub(
                ProgramSubscriptionV0 {
                    programs: vec![config.token_program_account],
                },
            )),
            regions: vec![],
        })
        .await
        .expect("subscribes to pending transactions by program id")
        .into_inner();

    print_next_leader_info(&mut client).await;
    print_packet_stream(&mut client, pending_transactions).await;
}

async fn print_next_leader_info(
    client: &mut SearcherServiceClient<InterceptedService<Channel, ClientInterceptor>>,
) {
    let next_leader = client
        .get_next_scheduled_leader(NextScheduledLeaderRequest {})
        .await
        .expect("gets next scheduled leader")
        .into_inner();
    println!(
        "next jito-solana slot in {} slots for leader {:?}",
        next_leader.next_leader_slot - next_leader.current_slot,
        next_leader.next_leader_identity
    );
}

async fn print_packet_stream(
    client: &mut SearcherServiceClient<InterceptedService<Channel, ClientInterceptor>>,
    mut pending_transactions: Streaming<PendingTxNotification>,
) {
    let bytes = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".as_bytes();
    let mut token_program_id_array = [0u8; 32];
    token_program_id_array.copy_from_slice(&bytes[..32]);
    let token_program_id = Pubkey::new_from_array(token_program_id_array);
    loop {
        match timeout(Duration::from_secs(5), pending_transactions.next()).await {
            Ok(Some(Ok(notification))) => {
                let transactions: Vec<VersionedTransaction> = notification
                    .transactions
                    .iter()
                    .filter_map(versioned_tx_from_packet)
                    .collect();
                for tx in transactions {
                    //info!("tx message {:?}", tx.message);
                    let transaction_accounts: Vec<Pubkey> =
                        tx.message.static_account_keys().iter().cloned().collect();
                    for instruction in tx.message.instructions().iter() {
                        let result = TokenInstruction::unpack(&instruction.data).map_err(|e| {
                            format!("Failed to decode SPL Token instruction: {:?}", e)
                        });
                        if let Ok(token_instruction) = result {
                            match token_instruction {
                                TokenInstruction::InitializeMint { decimals, mint_authority, freeze_authority } => {
                                    info!("tx sig: {:?}", tx.signatures[0]);
                                    info!("tx {:?}", tx.json_encode());
                                    info!("Token ins {:?}", token_instruction);
                                }
                                TokenInstruction::InitializeAccount => {},
                                TokenInstruction::InitializeMultisig { m } => {},
                                TokenInstruction::Transfer { amount } => {},
                                TokenInstruction::Approve { amount } => {},
                                TokenInstruction::Revoke => {},
                                TokenInstruction::SetAuthority { authority_type, new_authority } => {},
                                TokenInstruction::MintTo { amount } => {},
                                TokenInstruction::Burn { amount } => {},
                                TokenInstruction::CloseAccount => {},
                                TokenInstruction::FreezeAccount => {},
                                TokenInstruction::ThawAccount => {},
                                TokenInstruction::TransferChecked { amount, decimals } => {},
                                TokenInstruction::ApproveChecked { amount, decimals } => {},
                                TokenInstruction::MintToChecked { amount, decimals } => {},
                                TokenInstruction::BurnChecked { amount, decimals } => {},
                                TokenInstruction::InitializeAccount2 { owner } => {},
                                TokenInstruction::SyncNative => {},
                                TokenInstruction::InitializeAccount3 { owner } => {},
                                TokenInstruction::InitializeMultisig2 { m } => {},
                                TokenInstruction::InitializeMint2 { decimals, mint_authority, freeze_authority } => {},
                                TokenInstruction::GetAccountDataSize => {},
                                TokenInstruction::InitializeImmutableOwner => {},
                                TokenInstruction::AmountToUiAmount { amount } => {},
                                TokenInstruction::UiAmountToAmount { ui_amount } => {},
                            }
                        }
                        /*
                        let accounts: Vec<_> = instruction
                            .accounts
                            .iter()
                            .map(|&index| {
                                // Check if the index is within bounds
                                if index as usize >= tx.message.static_account_keys().len() {
                                    panic!("Index out of bounds");
                                }
                                tx.message.static_account_keys()[index as usize]
                            })
                            .collect();
                        info!("accounts {:?}", accounts); */
                        /*
                        let accounts: Vec<AccountMeta> = instruction
                            .accounts
                            .iter()
                            .map(|&index| AccountMeta::new(transaction_accounts[index as usize], true))  // Adjust the flags based on your requirements
                            .collect();
                        let program_instruction = Instruction::new_with_bytes(token_program_id, &instruction.data, accounts);
                        info!("ins {:?}", instruction);
                        info!("prog ins {:?}", program_instruction);
                        */
                    }
                }
            }
            Ok(Some(Err(e))) => {
                info!("error from pending transaction stream: {:?}", e);
                break;
            }
            Ok(None) => {
                info!("pending transaction stream closed");
                break;
            }
            Err(_) => {
                print_next_leader_info(client).await;
            }
        }
    }
}
