use serde::{Deserialize, Serialize};

macro_rules! string_identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub(crate) struct $name(pub(crate) String);

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl $name {
            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

string_identifier!(MediaId);
string_identifier!(MediaUri);
string_identifier!(ProviderId);
string_identifier!(ProviderInstanceId);
string_identifier!(QueueId);
string_identifier!(QueueItemId);
string_identifier!(PlayerId);
string_identifier!(UserId);
string_identifier!(AccessToken);
