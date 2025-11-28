'use strict';

const _ = require('lodash');

module.exports = ({ config }) => {
    const resolvers = {
        Query: {
            async substances(parent, args, ctx) {
                ctx.args = args; // Store for nested resolvers
                return ctx.pwService.getSubstances(args);
            },
            async effects_by_substance(parent, args, ctx) {
                ctx.args = args;
                return ctx.pwService.getSubstanceEffects(args);
            },
            async substances_by_effect(parent, args, ctx) {
                ctx.args = args;
                return ctx.pwService.getEffectSubstances(args);
            },
        },
        Substance: {
            async effects(parent, args, ctx) {
                const substance = parent.name;
                return ctx.pwService.getSubstanceEffects({
                    ...ctx.args,
                    substance
                });
            },
            async summary(parent, args, ctx) {
                return ctx.pwService.getSubstanceAbstract({ substance: parent.name });
            },
            async images(parent, args, ctx) {
                return ctx.pwService.getSubstanceImages({ substance: parent.name });
            },
            // Interaction Resolvers
            async uncertainInteractions(parent, args, ctx) {
                return resolveInteractions(parent.uncertainInteractions, ctx);
            },
            async unsafeInteractions(parent, args, ctx) {
                return resolveInteractions(parent.unsafeInteractions, ctx);
            },
            async dangerousInteractions(parent, args, ctx) {
                return resolveInteractions(parent.dangerousInteractions, ctx);
            }
        },
        Effect: {
            async substances(parent, args, ctx) {
                return ctx.pwService.getEffectSubstances({
                    ...ctx.args,
                    effect: parent.name
                });
            }
        }
    };

    // Add Plebiscite resolvers if enabled
    if (config.features.plebiscite.enabled) {
        resolvers.Query.erowid = async (parent, { substance, offset, limit }, ctx) => {
            return ctx.plebisciteDB.find({ substance, offset, limit });
        };
    }

    return resolvers;
};

async function resolveInteractions(interactions, ctx) {
    if (!Array.isArray(interactions)) return null;

    return Promise.all(interactions.map(async (name) => {
        const results = await ctx.pwService.getSubstances({
            query: name,
            limit: 1,
            offset: 0
        });

        if (results.length === 1) return results[0];
        return { name }; // Fallback if not found
    }));
}
