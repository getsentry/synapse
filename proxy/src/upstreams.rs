use crate::config::UpstreamConfig;
use crate::errors::ProxyError;
use http::uri::{Scheme, Uri};
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug)]
pub struct Upstream {
    pub scheme: Scheme,
    pub authority: String,
}

impl TryFrom<UpstreamConfig> for Upstream {
    type Error = ProxyError;
    fn try_from(config: UpstreamConfig) -> Result<Self, Self::Error> {
        let uri: Uri = config.url.parse()?;
        let scheme = uri.scheme().ok_or(ProxyError::InvalidUpstream)?.clone();
        let authority = uri
            .authority()
            .map(|a| a.to_string())
            .ok_or(ProxyError::InvalidUpstream)?;

        Ok(Self { scheme, authority })
    }
}

pub struct Upstreams {
    map: HashMap<String, Upstream>,
}

impl Upstreams {
    pub fn try_new(config: Vec<UpstreamConfig>) -> Result<Self, ProxyError> {
        let upstreams = config
            .into_iter()
            .map(|u| {
                let name = u.name.clone();
                Upstream::try_from(u).map(|up| (name, up))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        Ok(Upstreams { map: upstreams })
    }

    pub fn get(&self, upstream: &str) -> Option<&Upstream> {
        self.map.get(upstream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstreams() {
        let valid_config = UpstreamConfig {
            name: "getsentry-us".into(),
            url: "http://1.1.1.1:80".into(),
        };

        let invalid_config = UpstreamConfig {
            name: "getsentry-de".into(),
            url: "1.1.1.1:80".into(),
        };

        let upstream = Upstream::try_from(valid_config).expect("Valid upstream should parse");
        assert_eq!(upstream.scheme, Scheme::HTTP);
        assert_eq!(upstream.authority, "1.1.1.1:80");
        assert!(Upstream::try_from(invalid_config).is_err());
    }
}
