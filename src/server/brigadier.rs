use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use lazy_static::lazy_static;

use std::io::{Read, Write};

use crate::{
    chat::Text,
    util::{EncodingHelper, IOError, IOErrorKind, IOResult, VarInt},
    version::*,
};

use super::{
    packet_ids::ServerPacketType,
    packets::{Packet, ServerPacket},
};

#[derive(Debug, Clone)]
pub struct Suggestions {
    pub start: i32,
    pub length: i32,
    pub matches: Vec<Suggestion>,
}

impl Suggestions {
    pub fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self> {
        Ok(Self {
            start: VarInt::decode_simple(src)?.get(),
            length: VarInt::decode_simple(src)?.get(),
            matches: {
                let mut matches = Vec::new();
                for _ in 0..VarInt::decode_simple(src)?.get() {
                    matches.push(Suggestion {
                        text: EncodingHelper::read_string(src, 32767)?,
                        tooltip: if src.read_u8()? != 0 {
                            Some(EncodingHelper::read_text(src, version)?)
                        } else {
                            None
                        },
                    });
                }
                matches
            },
        })
    }

    pub fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        VarInt(self.start).encode_simple(dst)?;
        VarInt(self.length).encode_simple(dst)?;
        VarInt(self.matches.len() as i32).encode_simple(dst)?;
        for suggestion in &self.matches {
            EncodingHelper::write_string(dst, &suggestion.text)?;
            if let Some(ref tooltip) = suggestion.tooltip {
                dst.write_u8(1)?;
                EncodingHelper::write_text(dst, version, tooltip)?;
            } else {
                dst.write_u8(0)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub text: String,
    pub tooltip: Option<Text>,
}

#[derive(Debug, Clone)]
pub struct Commands {
    pub nodes: Vec<CommandNode>,
    pub root_index: usize,
}

impl ServerPacket for Commands {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::Commands
    }
}

impl Packet for Commands {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            nodes: (0..VarInt::decode_simple(src)?.get())
                .map(|_| CommandNode::decode(src, version))
                .collect::<IOResult<Vec<_>>>()?,
            root_index: VarInt::decode_simple(src)?.get() as usize,
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _version: i32) -> IOResult<()> {
        VarInt(self.nodes.len() as i32).encode_simple(dst)?;
        for node in &self.nodes {
            node.encode(dst)?;
        }
        VarInt(self.root_index as i32).encode_simple(dst)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CommandNode {
    pub childrens: Vec<usize>,
    pub node_type: CommandNodeType,
    pub executable: bool,
    pub redirect_index: Option<usize>,
}

impl CommandNode {
    pub fn encode<W: Write + ?Sized>(&self, dst: &mut W) -> IOResult<()> {
        let mut flags = match self.node_type {
            CommandNodeType::Root => 0,
            CommandNodeType::Literal(_) => 1,
            CommandNodeType::Argument { .. } => 2,
        };
        if self.executable {
            flags |= 0x04;
        }
        if self.redirect_index.is_some() {
            flags |= 0x08;
        }
        if let CommandNodeType::Argument {
            suggestions_type, ..
        } = &self.node_type
        {
            suggestions_type.is_some().then(|| flags |= 0x10);
        }
        dst.write_u8(flags)?;

        VarInt(self.childrens.len() as i32).encode_simple(dst)?;
        for children in &self.childrens {
            VarInt(*children as i32).encode_simple(dst)?;
        }
        if let Some(ref redirect_index) = self.redirect_index {
            VarInt(*redirect_index as i32).encode_simple(dst)?;
        }
        match &self.node_type {
            CommandNodeType::Root => {}
            CommandNodeType::Literal(name) => EncodingHelper::write_string(dst, name)?,
            CommandNodeType::Argument {
                name,
                parser_id,
                properties,
                suggestions_type,
            } => {
                EncodingHelper::write_string(dst, name)?;
                VarInt(*parser_id as i32).encode_simple(dst)?;
                if let Some(ref properties) = properties {
                    properties.encode(dst)?;
                }
                if let Some(ref suggestions_type) = suggestions_type {
                    EncodingHelper::write_string(
                        dst,
                        match suggestions_type {
                            SuggestionsType::AskServer => "minecraft:ask_server",
                            SuggestionsType::AllRecipes => "minecraft:all_recipes",
                            SuggestionsType::AvailableSounds => "minecraft:available_sounds",
                            SuggestionsType::SummonableEntities => "minecraft:summonable_entities",
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self> {
        let flags = src.read_u8()?;
        let node_type = flags & 0x03;
        let executable = flags & 0x04 != 0;
        let has_redirect = flags & 0x08 != 0;
        let has_suggestions = flags & 0x10 != 0;
        let childrens = (0..VarInt::decode_simple(src)?.get())
            .map(|_| VarInt::decode_simple(src).map(|x| x.get() as usize))
            .collect::<IOResult<Vec<_>>>()?;
        let redirect_index = match has_redirect {
            true => Some(VarInt::decode_simple(src)?.get() as usize),
            false => None,
        };
        let node_type = match node_type {
            0 => CommandNodeType::Root,
            1 => CommandNodeType::Literal(EncodingHelper::read_string(src, 32767)?),
            2 => {
                let name = EncodingHelper::read_string(src, 32767)?;
                let parser_id = VarInt::decode_simple(src)?.get() as usize;
                let properties = ArgumentProperty::decode_by_parser_id(src, parser_id, version)?;
                let suggestions_type = match has_suggestions {
                    true => Some(match EncodingHelper::read_string(src, 32767)?.as_str() {
                        "minecraft:ask_server" => SuggestionsType::AskServer,
                        "minecraft:all_recipes" => SuggestionsType::AllRecipes,
                        "minecraft:available_sounds" => SuggestionsType::AvailableSounds,
                        "minecraft:summonable_entities" => SuggestionsType::SummonableEntities,
                        _ => {
                            return Err(IOError::new(
                                IOErrorKind::InvalidData,
                                "Invalid suggestions type",
                            ))
                        }
                    }),
                    false => None,
                };
                CommandNodeType::Argument {
                    name,
                    parser_id,
                    properties,
                    suggestions_type,
                }
            }
            _ => return Err(IOError::new(IOErrorKind::InvalidData, "Invalid node type")),
        };
        Ok(Self {
            childrens,
            node_type,
            executable,
            redirect_index,
        })
    }
}

#[derive(Debug, Clone)]
pub enum CommandNodeType {
    Root,
    Literal(String),
    Argument {
        name: String,
        parser_id: usize,
        properties: Option<ArgumentProperty>,
        suggestions_type: Option<SuggestionsType>,
    },
}

#[derive(Debug, Clone)]
pub enum SuggestionsType {
    AskServer,
    AllRecipes,
    AvailableSounds,
    SummonableEntities,
}

#[derive(Debug, Clone)]
pub enum ArgumentProperty {
    Double { min: Option<f64>, max: Option<f64> },
    Float { min: Option<f32>, max: Option<f32> },
    Int { min: Option<i32>, max: Option<i32> },
    Long { min: Option<i64>, max: Option<i64> },
    String(StringParserType),
    Entity { mask: u8 },
    ScoreHolder { mask: u8 },
    Time { min: i32 },
    ResourceOrTag { registry: String },
    ResourceOrTagKey { registry: String },
    Resource { registry: String },
    ResourceKey { registry: String },
}

impl ArgumentProperty {
    pub fn encode<W: Write + ?Sized>(&self, dst: &mut W) -> IOResult<()> {
        match self {
            Self::Double { min, max } => {
                let mut flags = 0;
                min.is_some().then(|| flags |= 0x01);
                max.is_some().then(|| flags |= 0x02);
                dst.write_u8(flags)?;
                if let Some(min) = min {
                    dst.write_f64::<BE>(*min)?;
                }
                if let Some(max) = max {
                    dst.write_f64::<BE>(*max)?;
                }
            }
            Self::Float { min, max } => {
                let mut flags = 0;
                min.is_some().then(|| flags |= 0x01);
                max.is_some().then(|| flags |= 0x02);
                dst.write_u8(flags)?;
                if let Some(min) = min {
                    dst.write_f32::<BE>(*min)?;
                }
                if let Some(max) = max {
                    dst.write_f32::<BE>(*max)?;
                }
            }
            Self::Int { min, max } => {
                let mut flags = 0;
                min.is_some().then(|| flags |= 0x01);
                max.is_some().then(|| flags |= 0x02);
                dst.write_u8(flags)?;
                if let Some(min) = min {
                    dst.write_i32::<BE>(*min)?;
                }
                if let Some(max) = max {
                    dst.write_i32::<BE>(*max)?;
                }
            }
            Self::Long { min, max } => {
                let mut flags = 0;
                min.is_some().then(|| flags |= 0x01);
                max.is_some().then(|| flags |= 0x02);
                dst.write_u8(flags)?;
                if let Some(min) = min {
                    dst.write_i64::<BE>(*min)?;
                }
                if let Some(max) = max {
                    dst.write_i64::<BE>(*max)?;
                }
            }
            Self::String(parser_type) => {
                dst.write_u8(match parser_type {
                    StringParserType::SingleWord => 0,
                    StringParserType::QuotablePhrase => 1,
                    StringParserType::GreedyPhrase => 2,
                })?;
            }
            Self::Entity { mask } => dst.write_u8(*mask)?,
            Self::ScoreHolder { mask } => dst.write_u8(*mask)?,
            Self::Time { min } => dst.write_i32::<BE>(*min)?,
            Self::ResourceOrTag { registry } => EncodingHelper::write_string(dst, registry)?,
            Self::ResourceOrTagKey { registry } => EncodingHelper::write_string(dst, registry)?,
            Self::Resource { registry } => EncodingHelper::write_string(dst, registry)?,
            Self::ResourceKey { registry } => EncodingHelper::write_string(dst, registry)?,
        }
        Ok(())
    }

    pub fn decode_double<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        let flags = src.read_u8()?;
        let min = if flags & 0x01 != 0 {
            Some(src.read_f64::<BE>()?)
        } else {
            None
        };
        let max = if flags & 0x02 != 0 {
            Some(src.read_f64::<BE>()?)
        } else {
            None
        };
        Ok(Some(Self::Double { min, max }))
    }

    pub fn decode_float<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        let flags = src.read_u8()?;
        let min = if flags & 0x01 != 0 {
            Some(src.read_f32::<BE>()?)
        } else {
            None
        };
        let max = if flags & 0x02 != 0 {
            Some(src.read_f32::<BE>()?)
        } else {
            None
        };
        Ok(Some(Self::Float { min, max }))
    }

    pub fn decode_int<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        let flags = src.read_u8()?;
        let min = if flags & 0x01 != 0 {
            Some(src.read_i32::<BE>()?)
        } else {
            None
        };
        let max = if flags & 0x02 != 0 {
            Some(src.read_i32::<BE>()?)
        } else {
            None
        };
        Ok(Some(Self::Int { min, max }))
    }

    pub fn decode_long<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        let flags = src.read_u8()?;
        let min = if flags & 0x01 != 0 {
            Some(src.read_i64::<BE>()?)
        } else {
            None
        };
        let max = if flags & 0x02 != 0 {
            Some(src.read_i64::<BE>()?)
        } else {
            None
        };
        Ok(Some(Self::Long { min, max }))
    }

    pub fn decode_string<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(match src.read_u8()? {
            0 => Self::String(StringParserType::SingleWord),
            1 => Self::String(StringParserType::QuotablePhrase),
            2 => Self::String(StringParserType::GreedyPhrase),
            _ => {
                return Err(IOError::new(
                    IOErrorKind::InvalidData,
                    "Invalid string parser type",
                ))
            }
        }))
    }

    fn decode_entity<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::Entity {
            mask: src.read_u8()?,
        }))
    }

    fn decode_score_holder<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::ScoreHolder {
            mask: src.read_u8()?,
        }))
    }

    fn decode_time<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::Time {
            min: src.read_i32::<BE>()?,
        }))
    }

    fn decode_resource_or_tag<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::ResourceOrTag {
            registry: EncodingHelper::read_string(src, 32767)?,
        }))
    }

    fn decode_resource_or_tag_key<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::ResourceOrTagKey {
            registry: EncodingHelper::read_string(src, 32767)?,
        }))
    }

    fn decode_resource<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::Resource {
            registry: EncodingHelper::read_string(src, 32767)?,
        }))
    }

    fn decode_resource_key<R: Read + ?Sized>(src: &mut R) -> IOResult<Option<Self>> {
        Ok(Some(Self::ResourceKey {
            registry: EncodingHelper::read_string(src, 32767)?,
        }))
    }

    fn decode_nothing<R: Read + ?Sized>(_src: &mut R) -> IOResult<Option<Self>> {
        Ok(None)
    }

    pub fn decode_by_parser_id<R: Read + ?Sized>(
        src: &mut R,
        parser_id: usize,
        version: i32,
    ) -> IOResult<Option<Self>> {
        let decoder_list = if version >= R1_19 {
            if version >= R1_20_5 {
                ARGUMENT_PROPERTY_DECODERS_1_20_5.as_slice()
            } else if version >= R1_20_3 {
                ARGUMENT_PROPERTY_DECODERS_1_20_3.as_slice()
            } else if version >= R1_19_4 {
                ARGUMENT_PROPERTY_DECODERS_1_19_4.as_slice()
            } else if version >= R1_19_3 {
                ARGUMENT_PROPERTY_DECODERS_1_19_3.as_slice()
            } else {
                ARGUMENT_PROPERTY_DECODERS_1_19.as_slice()
            }
        } else {
            ARGUMENT_PROPERTY_DECODERS.as_slice()
        };
        if parser_id >= decoder_list.len() {
            return Err(IOError::new(IOErrorKind::InvalidData, "Invalid parser id"));
        }
        let (_, decoder) = &decoder_list[parser_id];
        struct ReadWrapper<'a, R: ?Sized> {
            inner: &'a mut R,
        }
        impl<'a, R: Read + ?Sized> Read for ReadWrapper<'a, R> {
            fn read(&mut self, buf: &mut [u8]) -> IOResult<usize> {
                self.inner.read(buf)
            }
        }
        let mut wrapper = ReadWrapper { inner: src };
        decoder(unsafe { core::mem::transmute(&mut wrapper as &mut dyn Read) })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StringParserType {
    SingleWord,
    QuotablePhrase,
    GreedyPhrase,
}

macro_rules! register_all {
    ($(($id:literal, $func:ident))*) => {{
        let mut list: Vec<(String, DecoderFunc)> = Vec::new();
        $(
            list.push(($id.to_string(), ArgumentProperty::$func::<(dyn Read + 'static)>));
        )*
        list
    }};
}

type DecoderFunc = fn(&mut (dyn Read + 'static)) -> IOResult<Option<ArgumentProperty>>;

lazy_static! {

    static ref ARGUMENT_PROPERTY_DECODERS: Vec<(String, DecoderFunc)> = {
        register_all! {
            ("brigadier:bool", decode_nothing)
            ("brigadier:float", decode_float)
            ("brigadier:double", decode_double)
            ("brigadier:integer", decode_int)
            ("brigadier:long", decode_long)
            ("brigadier:string", decode_string)
            ("minecraft:entity", decode_entity)
            ("minecraft:game_profile", decode_nothing)
            ("minecraft:block_pos", decode_nothing)
            ("minecraft:column_pos", decode_nothing)
            ("minecraft:vec3", decode_nothing)
            ("minecraft:vec2", decode_nothing)
            ("minecraft:block_state", decode_nothing)
            ("minecraft:block_predicate", decode_nothing)
            ("minecraft:item_stack", decode_nothing)
            ("minecraft:item_predicate", decode_nothing)
            ("minecraft:color", decode_nothing)
            ("minecraft:component", decode_nothing)
            ("minecraft:message", decode_nothing)
            ("minecraft:nbt_compound_tag", decode_nothing) // 1.14
            ("minecraft:nbt_tag", decode_nothing) // 1.14
            ("minecraft:nbt_path", decode_nothing)
            ("minecraft:objective", decode_nothing)
            ("minecraft:objective_criteria", decode_nothing)
            ("minecraft:operation", decode_nothing)
            ("minecraft:particle", decode_nothing)
            ("minecraft:angle", decode_nothing) // 1.16.2
            ("minecraft:rotation", decode_nothing)
            ("minecraft:scoreboard_slot", decode_nothing)
            ("minecraft:score_holder", decode_score_holder)
            ("minecraft:swizzle", decode_nothing)
            ("minecraft:team", decode_nothing)
            ("minecraft:item_slot", decode_nothing)
            ("minecraft:resource_location", decode_nothing)
            ("minecraft:mob_effect", decode_nothing)
            ("minecraft:function", decode_nothing)
            ("minecraft:entity_anchor", decode_nothing)
            ("minecraft:int_range", decode_nothing)
            ("minecraft:float_range", decode_nothing)
            ("minecraft:item_enchantment", decode_nothing)
            ("minecraft:entity_summon", decode_nothing)
            ("minecraft:dimension", decode_nothing)
            ("minecraft:time", decode_nothing) // 1.14
            ("minecraft:resource_or_tag", decode_resource_or_tag) // 1.18.2
            ("minecraft:resource", decode_resource) // 1.18.2
            ("minecraft:uuid", decode_nothing) // 1.16
            ("minecraft:nbt", decode_nothing) // 1.13 // removed
        }
    };

    static ref ARGUMENT_PROPERTY_DECODERS_1_19: Vec<(String, DecoderFunc)> = {
        register_all! {
            ("brigadier:bool", decode_nothing)
            ("brigadier:float", decode_float)
            ("brigadier:double", decode_double)
            ("brigadier:integer", decode_int)
            ("brigadier:long", decode_long)
            ("brigadier:string", decode_string)
            ("minecraft:entity", decode_entity)
            ("minecraft:game_profile", decode_nothing)
            ("minecraft:block_pos", decode_nothing)
            ("minecraft:column_pos", decode_nothing)
            ("minecraft:vec3", decode_nothing)
            ("minecraft:vec2", decode_nothing)
            ("minecraft:block_state", decode_nothing)
            ("minecraft:block_predicate", decode_nothing)
            ("minecraft:item_stack", decode_nothing)
            ("minecraft:item_predicate", decode_nothing)
            ("minecraft:color", decode_nothing)
            ("minecraft:component", decode_nothing)
            ("minecraft:message", decode_nothing)
            ("minecraft:nbt_compound_tag", decode_nothing)
            ("minecraft:nbt_tag", decode_nothing)
            ("minecraft:nbt_path", decode_nothing)
            ("minecraft:objective", decode_nothing)
            ("minecraft:objective_criteria", decode_nothing)
            ("minecraft:operation", decode_nothing)
            ("minecraft:particle", decode_nothing)
            ("minecraft:angle", decode_nothing)
            ("minecraft:rotation", decode_nothing)
            ("minecraft:scoreboard_slot", decode_nothing)
            ("minecraft:score_holder", decode_score_holder)
            ("minecraft:swizzle", decode_nothing)
            ("minecraft:team", decode_nothing)
            ("minecraft:item_slot", decode_nothing)
            ("minecraft:resource_location", decode_nothing)
            ("minecraft:mob_effect", decode_nothing)
            ("minecraft:function", decode_nothing)
            ("minecraft:entity_anchor", decode_nothing)
            ("minecraft:int_range", decode_nothing)
            ("minecraft:float_range", decode_nothing)
            ("minecraft:item_enchantment", decode_nothing)
            ("minecraft:entity_summon", decode_nothing)
            ("minecraft:dimension", decode_nothing)
            ("minecraft:time", decode_nothing)
            ("minecraft:resource_or_tag", decode_resource_or_tag)
            ("minecraft:resource", decode_resource)
            ("minecraft:template_mirror", decode_nothing)
            ("minecraft:template_rotation", decode_nothing)
            ("minecraft:uuid", decode_nothing)
        }
    };

    static ref ARGUMENT_PROPERTY_DECODERS_1_19_3: Vec<(String, DecoderFunc)> = {
        register_all! {
            ("brigadier:bool", decode_nothing)
            ("brigadier:float", decode_float)
            ("brigadier:double", decode_double)
            ("brigadier:integer", decode_int)
            ("brigadier:long", decode_long)
            ("brigadier:string", decode_string)
            ("minecraft:entity", decode_entity)
            ("minecraft:game_profile", decode_nothing)
            ("minecraft:block_pos", decode_nothing)
            ("minecraft:column_pos", decode_nothing)
            ("minecraft:vec3", decode_nothing)
            ("minecraft:vec2", decode_nothing)
            ("minecraft:block_state", decode_nothing)
            ("minecraft:block_predicate", decode_nothing)
            ("minecraft:item_stack", decode_nothing)
            ("minecraft:item_predicate", decode_nothing)
            ("minecraft:color", decode_nothing)
            ("minecraft:component", decode_nothing)
            ("minecraft:message", decode_nothing)
            ("minecraft:nbt_compound_tag", decode_nothing)
            ("minecraft:nbt_tag", decode_nothing)
            ("minecraft:nbt_path", decode_nothing)
            ("minecraft:objective", decode_nothing)
            ("minecraft:objective_criteria", decode_nothing)
            ("minecraft:operation", decode_nothing)
            ("minecraft:particle", decode_nothing)
            ("minecraft:angle", decode_nothing)
            ("minecraft:rotation", decode_nothing)
            ("minecraft:scoreboard_slot", decode_nothing)
            ("minecraft:score_holder", decode_score_holder)
            ("minecraft:swizzle", decode_nothing)
            ("minecraft:team", decode_nothing)
            ("minecraft:item_slot", decode_nothing)
            ("minecraft:resource_location", decode_nothing)
            ("minecraft:function", decode_nothing)
            ("minecraft:entity_anchor", decode_nothing)
            ("minecraft:int_range", decode_nothing)
            ("minecraft:float_range", decode_nothing)
            ("minecraft:dimension", decode_nothing)
            ("minecraft:gamemode", decode_nothing)
            ("minecraft:time", decode_nothing)
            ("minecraft:resource_or_tag", decode_resource_or_tag)
            ("minecraft:resource_or_tag_key", decode_resource_or_tag_key)
            ("minecraft:resource", decode_resource)
            ("minecraft:resource_key", decode_resource_key)
            ("minecraft:template_mirror", decode_nothing)
            ("minecraft:template_rotation", decode_nothing)
            ("minecraft:uuid", decode_nothing)
        }
    };

    static ref ARGUMENT_PROPERTY_DECODERS_1_19_4: Vec<(String, DecoderFunc)> = {
        register_all! {
            ("brigadier:bool", decode_nothing)
            ("brigadier:float", decode_float)
            ("brigadier:double", decode_double)
            ("brigadier:integer", decode_int)
            ("brigadier:long", decode_long)
            ("brigadier:string", decode_string)
            ("minecraft:entity", decode_entity)
            ("minecraft:game_profile", decode_nothing)
            ("minecraft:block_pos", decode_nothing)
            ("minecraft:column_pos", decode_nothing)
            ("minecraft:vec3", decode_nothing)
            ("minecraft:vec2", decode_nothing)
            ("minecraft:block_state", decode_nothing)
            ("minecraft:block_predicate", decode_nothing)
            ("minecraft:item_stack", decode_nothing)
            ("minecraft:item_predicate", decode_nothing)
            ("minecraft:color", decode_nothing)
            ("minecraft:component", decode_nothing)
            ("minecraft:message", decode_nothing)
            ("minecraft:nbt_compound_tag", decode_nothing)
            ("minecraft:nbt_tag", decode_nothing)
            ("minecraft:nbt_path", decode_nothing)
            ("minecraft:objective", decode_nothing)
            ("minecraft:objective_criteria", decode_nothing)
            ("minecraft:operation", decode_nothing)
            ("minecraft:particle", decode_nothing)
            ("minecraft:angle", decode_nothing)
            ("minecraft:rotation", decode_nothing)
            ("minecraft:scoreboard_slot", decode_nothing)
            ("minecraft:score_holder", decode_score_holder)
            ("minecraft:swizzle", decode_nothing)
            ("minecraft:team", decode_nothing)
            ("minecraft:item_slot", decode_nothing)
            ("minecraft:resource_location", decode_nothing)
            ("minecraft:function", decode_nothing)
            ("minecraft:entity_anchor", decode_nothing)
            ("minecraft:int_range", decode_nothing)
            ("minecraft:float_range", decode_nothing)
            ("minecraft:dimension", decode_nothing)
            ("minecraft:gamemode", decode_nothing)
            ("minecraft:time", decode_time)
            ("minecraft:resource_or_tag", decode_resource_or_tag)
            ("minecraft:resource_or_tag_key", decode_resource_or_tag_key)
            ("minecraft:resource", decode_resource)
            ("minecraft:resource_key", decode_resource_key)
            ("minecraft:template_mirror", decode_nothing)
            ("minecraft:template_rotation", decode_nothing)
            ("minecraft:uuid", decode_nothing)
            ("minecraft:heightmap", decode_nothing)
        }
    };

    static ref ARGUMENT_PROPERTY_DECODERS_1_20_3: Vec<(String, DecoderFunc)> = {
        register_all! {
            ("brigadier:bool", decode_nothing)
            ("brigadier:float", decode_float)
            ("brigadier:double", decode_double)
            ("brigadier:integer", decode_int)
            ("brigadier:long", decode_long)
            ("brigadier:string", decode_string)
            ("minecraft:entity", decode_entity)
            ("minecraft:game_profile", decode_nothing)
            ("minecraft:block_pos", decode_nothing)
            ("minecraft:column_pos", decode_nothing)
            ("minecraft:vec3", decode_nothing)
            ("minecraft:vec2", decode_nothing)
            ("minecraft:block_state", decode_nothing)
            ("minecraft:block_predicate", decode_nothing)
            ("minecraft:item_stack", decode_nothing)
            ("minecraft:item_predicate", decode_nothing)
            ("minecraft:color", decode_nothing)
            ("minecraft:component", decode_nothing)
            ("minecraft:style", decode_nothing)
            ("minecraft:message", decode_nothing)
            ("minecraft:nbt_compound_tag", decode_nothing)
            ("minecraft:nbt_tag", decode_nothing)
            ("minecraft:nbt_path", decode_nothing)
            ("minecraft:objective", decode_nothing)
            ("minecraft:objective_criteria", decode_nothing)
            ("minecraft:operation", decode_nothing)
            ("minecraft:particle", decode_nothing)
            ("minecraft:angle", decode_nothing)
            ("minecraft:rotation", decode_nothing)
            ("minecraft:scoreboard_slot", decode_nothing)
            ("minecraft:score_holder", decode_score_holder)
            ("minecraft:swizzle", decode_nothing)
            ("minecraft:team", decode_nothing)
            ("minecraft:item_slot", decode_nothing)
            ("minecraft:resource_location", decode_nothing)
            ("minecraft:function", decode_nothing)
            ("minecraft:entity_anchor", decode_nothing)
            ("minecraft:int_range", decode_nothing)
            ("minecraft:float_range", decode_nothing)
            ("minecraft:dimension", decode_nothing)
            ("minecraft:gamemode", decode_nothing)
            ("minecraft:time", decode_time)
            ("minecraft:resource_or_tag", decode_resource_or_tag)
            ("minecraft:resource_or_tag_key", decode_resource_or_tag_key)
            ("minecraft:resource", decode_resource)
            ("minecraft:resource_key", decode_resource_key)
            ("minecraft:template_mirror", decode_nothing)
            ("minecraft:template_rotation", decode_nothing)
            ("minecraft:uuid", decode_nothing)
            ("minecraft:heightmap", decode_nothing)
        }
    };

    static ref ARGUMENT_PROPERTY_DECODERS_1_20_5: Vec<(String, DecoderFunc)> = {
        register_all! {
            ("brigadier:bool", decode_nothing)
            ("brigadier:float", decode_float)
            ("brigadier:double", decode_double)
            ("brigadier:integer", decode_int)
            ("brigadier:long", decode_long)
            ("brigadier:string", decode_string)
            ("minecraft:entity", decode_entity)
            ("minecraft:game_profile", decode_nothing)
            ("minecraft:block_pos", decode_nothing)
            ("minecraft:column_pos", decode_nothing)
            ("minecraft:vec3", decode_nothing)
            ("minecraft:vec2", decode_nothing)
            ("minecraft:block_state", decode_nothing)
            ("minecraft:block_predicate", decode_nothing)
            ("minecraft:item_stack", decode_nothing)
            ("minecraft:item_predicate", decode_nothing)
            ("minecraft:color", decode_nothing)
            ("minecraft:component", decode_nothing)
            ("minecraft:style", decode_nothing)
            ("minecraft:message", decode_nothing)
            ("minecraft:nbt_compound_tag", decode_nothing)
            ("minecraft:nbt_tag", decode_nothing)
            ("minecraft:nbt_path", decode_nothing)
            ("minecraft:objective", decode_nothing)
            ("minecraft:objective_criteria", decode_nothing)
            ("minecraft:operation", decode_nothing)
            ("minecraft:particle", decode_nothing)
            ("minecraft:angle", decode_nothing)
            ("minecraft:rotation", decode_nothing)
            ("minecraft:scoreboard_slot", decode_nothing)
            ("minecraft:score_holder", decode_score_holder)
            ("minecraft:swizzle", decode_nothing)
            ("minecraft:team", decode_nothing)
            ("minecraft:item_slot", decode_nothing)
            ("minecraft:item_slots", decode_nothing)
            ("minecraft:resource_location", decode_nothing)
            ("minecraft:function", decode_nothing)
            ("minecraft:entity_anchor", decode_nothing)
            ("minecraft:int_range", decode_nothing)
            ("minecraft:float_range", decode_nothing)
            ("minecraft:dimension", decode_nothing)
            ("minecraft:gamemode", decode_nothing)
            ("minecraft:time", decode_time)
            ("minecraft:resource_or_tag", decode_resource_or_tag)
            ("minecraft:resource_or_tag_key", decode_resource_or_tag_key)
            ("minecraft:resource", decode_resource)
            ("minecraft:resource_key", decode_resource_key)
            ("minecraft:template_mirror", decode_nothing)
            ("minecraft:template_rotation", decode_nothing)
            ("minecraft:uuid", decode_nothing)
            ("minecraft:heightmap", decode_nothing)
            ("minecraft:loot_table", decode_nothing)
            ("minecraft:loot_predicate", decode_nothing)
            ("minecraft:loot_modifier", decode_nothing)
        }
    };
}
