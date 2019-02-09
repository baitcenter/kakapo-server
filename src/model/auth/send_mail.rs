use data::auth::InvitationToken;
use data::auth::Invitation;
use std::fmt::Debug;
use model::state::ActionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailError;

pub trait EmailSender {
    fn send_email(&self, invitation_token: InvitationToken) -> Result<Invitation, EmailError>;
}

impl EmailSender for ActionState {
    fn send_email(&self, invitation_token: InvitationToken) -> Result<Invitation, EmailError> {

        let inviatation = Invitation {
            email: invitation_token.email,
            expires_at: invitation_token.expires_at,
        };

        Ok(inviatation)
    }
}