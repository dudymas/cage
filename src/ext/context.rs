//! Extension methods for `compose_yml::v2::Service`.

use compose_yml::v2 as dc;
use std::path::Path;
use url;

use errors::*;
use util::ToStrOrErr;

/// These methods will appear as regular methods on `Context` in any module
/// which includes `ContextExt`.
pub trait ContextExt {
    /// Construct a short, easy-to-type alias for this `Context`, suitable
    /// for use as a command-line argument or a directory name.
    fn human_alias(&self) -> Result<String>;
}

impl ContextExt for dc::Context {
    fn human_alias(&self) -> Result<String> {
        match *self {
            dc::Context::GitUrl(ref git_url) => {
                // Convert a regular URL so we can parse it.
                let url: url::Url = try!(git_url.to_url());

                // Get the last component of the path.
                //
                // TODO LOW: We may need to unescape the path.
                let url_path = Path::new(url.path()).to_owned();
                let file_stem = try!(url_path.file_stem()
                    .ok_or_else(|| err!("Can't get repo name from {}", &git_url)));
                let base_alias = try!(file_stem.to_str_or_err()).to_owned();

                // Get the branch.  If available, this will be stored in the query.
                match url.fragment() {
                    None => Ok(base_alias),
                    Some(branch) => Ok(format!("{}_{}", base_alias, branch)),
                }
            }

            dc::Context::Dir(ref path) => {
                let file_stem = try!(path.file_stem()
                    .ok_or_else(|| {
                        err!("Can't get repo name from {}", &path.display())
                    }));
                Ok(try!(file_stem.to_str_or_err()).to_owned())
            }
        }
    }
}

#[test]
fn human_alias_uses_dir_name_and_branch() {
    let master = dc::Context::new("https://github.com/faradayio/rails_hello.git");
    assert_eq!(master.human_alias().unwrap(), "rails_hello");

    let branch = dc::Context::new("https://github.com/faradayio/rails_hello.git#dev");
    assert_eq!(branch.human_alias().unwrap(), "rails_hello_dev");

    let local = dc::Context::new("../src/node_hello");
    assert_eq!(local.human_alias().unwrap(), "node_hello");
}
