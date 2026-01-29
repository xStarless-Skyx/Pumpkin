use pumpkin_protocol::java::client::play::{
    ArgumentType, StringProtoArgBehavior, SuggestionProviders,
};

use pumpkin::{
    command::{
        args::{
            Arg, ArgumentConsumer, ConsumeResult, ConsumedArgs, FindArg, GetClientSideArgParser,
        },
        dispatcher::CommandError,
        CommandSender,
        tree::RawArgs,
    },
    server::Server,
};

/// Single-word string argument that enables server-side tab completion
/// via SuggestionProviders::AskServer
pub struct AskServerStringArgConsumer;

impl GetClientSideArgParser for AskServerStringArgConsumer {
    fn get_client_side_parser(&self) -> ArgumentType<'_> {
        ArgumentType::String(StringProtoArgBehavior::SingleWord)
    }

    fn get_client_side_suggestion_type_override(&self) -> Option<SuggestionProviders> {
        Some(SuggestionProviders::AskServer)
    }
}

impl ArgumentConsumer for AskServerStringArgConsumer {
    fn consume<'a, 'b>(
        &'a self,
        _sender: &'a CommandSender,
        _server: &'a Server,
        args: &'b mut RawArgs<'a>,
    ) -> ConsumeResult<'a> {
        let s_opt: Option<&'a str> = args.pop();
        Box::pin(async move { s_opt.map(Arg::Simple) })
    }
}

impl<'a> FindArg<'a> for AskServerStringArgConsumer {
    type Data = &'a str;

    fn find_arg(args: &'a ConsumedArgs, name: &str) -> Result<Self::Data, CommandError> {
        match args.get(name) {
            Some(Arg::Simple(data)) => Ok(data),
            _ => Err(CommandError::InvalidConsumption(Some(name.to_string()))),
        }
    }
}
