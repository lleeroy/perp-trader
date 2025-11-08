use crate::model::account::Account;
use mongodb::{
    bson::doc,
    error::Error,
    options::ClientOptions,
    Client, Collection,
    Database as MongoDatabase,
};

use tokio::sync::OnceCell;
use std::{sync::Arc, time::Duration};
use anyhow::{Context, Result};

/// Global database instance using OnceCell for lazy initialization
static DB_INSTANCE: OnceCell<Arc<Database>> = OnceCell::const_new();

/// Database structure for storing trading data and developer profiles
#[derive(Clone)]
#[allow(unused)]
pub struct Database {
    /// MongoDB client with connection pooling
    client: Arc<Client>,
    /// MongoDB database instance
    database: Arc<MongoDatabase>,
    /// Collection for storing developers
    developers: Arc<Collection<Account>>,
}

impl Database {
    /// Gets the global database instance, initializing it if necessary
    ///
    /// # Returns
    /// * `Result<Arc<Self>>` - Global database instance if successful, error if connection fails
    pub async fn get_instance() -> Result<Arc<Self>> {
        DB_INSTANCE
            .get_or_try_init(|| async {
                Self::new_internal().await.map(|db| Arc::new(db))
            })
            .await
            .map(|db| db.clone())
    }

    /// Creates a new TradingDatabase instance connected to MongoDB with connection pooling
    ///
    /// # Returns
    /// * `Result<Self>` - New TradingDatabase instance if successful, error if connection fails
    async fn new_internal() -> Result<Self> {
        dotenv::dotenv().ok();
        let database_password = std::env::var("DATABASE_PASSWORD")
            .context("Failed to get DATABASE_PASSWORD from environment variables")?;
        
        let client_url = format!(
            "mongodb+srv://***REMOVED***:{}@***REMOVED***/",
            database_password
        );

        // Configure connection pooling
        let mut client_options = ClientOptions::parse(client_url).await?;
        
        // Set connection pool options for better performance
        client_options.max_pool_size = Some(50); // Maximum number of connections in the pool
        client_options.min_pool_size = Some(5);  // Minimum number of connections to maintain
        client_options.max_connecting = Some(10); // Maximum number of connections being established
        client_options.connect_timeout = Some(Duration::from_secs(10)); // Connection timeout
        client_options.server_selection_timeout = Some(Duration::from_secs(10)); // Server selection timeout
        
        let client = Client::with_options(client_options)?;
        
        // Test the connection
        client.list_database_names().await?;
        
        let database = client.database("database");
        let developers = database.collection::<Account>("accounts");
        
        info!("Database connection established with connection pooling");
        
        Ok(Database {
            client: Arc::new(client),
            database: Arc::new(database),
            developers: Arc::new(developers),
        })
    }


    #[allow(unused)]
    pub async fn get_account_by_id(&self, wallet_id: u32) -> Result<Option<Account>, Error> {
        let filter = doc! {"wallet_id": wallet_id};
        match self.developers.find_one(filter).await {
            Ok(Some(account)) => Ok(Some(account)),
            Ok(None) => Ok(None),
            Err(e) => {
                error!("Error while querying the database: {}", e);
                Err(e)
            }
        }
    }

    /// Gets the wallet key by wallet id
    ///
    /// # Arguments
    /// * `wallet_id` - The wallet id
    ///
    /// # Returns
    /// * `Result<Option<String>, Error>` - The wallet key if found, None if not found, error if query fails
    #[allow(unused)]
    pub async fn get_key_by_id(&self, wallet_id: u32) -> Result<Option<String>, Error> {
        let filter = doc! {"wallet_id": wallet_id};
        match self.developers.find_one(filter).await {
            Ok(Some(account)) => Ok(Some(account.wallet_key)),
            Ok(None) => Ok(None),
            Err(e) => {
                error!("Error while querying the database: {}", e);
                Err(e)
            }
        }
    }
}