use futures::prelude::*;
use futures::future::{err, Either};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct KeycloakClientConfig {
    base_url: reqwest::Url,
}

impl KeycloakClientConfig {
    pub fn new(base_url: &str, realm: &str) -> Result<Self, reqwest::UrlError> {
        Ok(Self {
            base_url: reqwest::Url::parse(base_url)?.join(&format!("admin/realms/{}/", realm))?,
        })
    }
}


#[derive(Clone, Debug)]
pub struct KeycloakClient {
    config: KeycloakClientConfig,
    client: reqwest::r#async::Client,
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_timestamp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(rename = "emailVerified", skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(rename = "firstName", skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(rename = "lastName", skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(rename = "realmRoles", skip_serializing_if = "Option::is_none")]
    pub realm_roles: Option<Vec<String>>,
    #[serde(rename = "clientRoles", skip_serializing_if = "Option::is_none")]
    pub client_roles: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, Vec<String>>>,
    #[serde(skip)]
    _client: Option<KeycloakClient>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Role {
    pub id: uuid::Uuid,
    pub name: String,
}

impl User {
    pub fn update<'a>(&self, token: &str) -> impl Future<Item=(), Error=actix_web::Error> + 'a {
        let client = self._client.clone().unwrap();

        match client.config.base_url.join(&format!("users/{}", self.id.to_string())) {
            Ok(u) => Either::A(
                client.client.put(u)
                    .json(self)
                    .bearer_auth(token)
                    .send()
                    .and_then(|c| c.error_for_status())
                    .map(|_| ())
                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
            ),
            Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
        }
    }

    pub fn add_role<'a>(mut self, new_roles: &'a [&'a str], token: String) -> impl Future<Item=Self, Error=actix_web::Error> + 'a {
        let client = self._client.clone().unwrap();

        match client.config.base_url.join(&format!("users/{}/role-mappings/realm/available", self.id.to_string())) {
            Ok(u) => Either::A(
                client.client.get(u)
                    .bearer_auth(token.clone())
                    .send()
                    .and_then(|c| c.error_for_status())
                    .and_then(|mut c| c.json::<Vec<Role>>())
                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                    .and_then(move |roles| {
                        let roles_to_add = new_roles.into_iter().map(|r1| {
                            match roles.iter().filter(|r2| {
                                r2.name == r1.to_string()
                            }).next() {
                                Some(r) => Some(r.to_owned()),
                                None => None
                            }
                        })
                            .filter(|r| Option::is_some(r))
                            .map(|r| r.unwrap())
                            .collect::<Vec<Role>>();

                        match client.config.base_url.join(&format!("users/{}/role-mappings/realm", self.id.to_string())) {
                            Ok(u) => Either::A(
                                client.client.post(u)
                                    .json(&roles_to_add)
                                    .bearer_auth(token)
                                    .send()
                                    .and_then(|c| c.error_for_status())
                                    .map(move |_| {
                                        let realm_roles = match self.realm_roles.as_mut() {
                                            Some(m) => m,
                                            None => {
                                                let roles = vec![];
                                                self.realm_roles = Some(roles);
                                                self.realm_roles.as_mut().unwrap()
                                            }
                                        };
                                        realm_roles.append(&mut new_roles.into_iter().map(|s| s.to_string()).collect());
                                        self
                                    })
                                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
                            ),
                            Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
                        }
                    })
            ),
            Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
        }
    }

    pub fn set_attribute(&mut self, attr: &str, value: &str) {
        let attributes = match self.attributes.as_mut() {
            Some(m) => m,
            None => {
                let map = HashMap::new();
                self.attributes = Some(map);
                self.attributes.as_mut().unwrap()
            }
        };
        attributes.insert(attr.to_owned(), vec![value.to_owned()]);
    }

    pub fn has_attribute(&self, attr: &str) -> bool {
        match &self.attributes {
            Some(a) => a.contains_key(attr),
            None => false
        }
    }
}

impl KeycloakClient {
    pub fn new(config: KeycloakClientConfig) -> Self {
        let mut d_headers = reqwest::header::HeaderMap::new();
        d_headers.insert(reqwest::header::CONTENT_TYPE, "application/json".parse().unwrap());

        Self {
            config,
            client: reqwest::r#async::Client::builder()
                .default_headers(d_headers)
                .build()
                .unwrap(),
        }
    }

    pub fn get_user<'a>(self, user_id: uuid::Uuid, token: &str) -> impl Future<Item=User, Error=actix_web::Error> + 'a {
        let client = self.client.clone();

        match self.config.base_url.join(&format!("users/{}", user_id.to_string())) {
            Ok(u) => Either::A(
                client.get(u)
                    .bearer_auth(token)
                    .send()
                    .and_then(|c| c.error_for_status())
                    .and_then(|mut c| c.json::<User>())
                    .map(|mut u| {
                        u._client = Some(self);
                        u
                    })
                    .map_err(|e| actix_web::error::ErrorInternalServerError(e))
            ),
            Err(e) => Either::B(err(actix_web::error::ErrorInternalServerError(e)))
        }
    }
}