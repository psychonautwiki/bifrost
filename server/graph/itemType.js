'use strict';

const _ = require('lodash');

const {
    makeExecutableSchema,
    SchemaDirectiveVisitor,
} = require('graphql-tools');

const schema = require('./schema/rootQuery');

const features = require('../util/features');

const baseResolvers = {
    Query: {
        async substances(data, args, ctx) {
            ctx.args = args;

            return ctx.substances.getSubstances(args);
        },
        async effects(data, args, ctx) {
            ctx.args = args;

            return ctx.substances.getEffects(args);
        },
        async effects_by_substance(data, args, ctx) {
            ctx.args = args;

            return ctx.substances.getSubstanceEffects(args);
        },
        async substances_by_effect(data, args, ctx) {
            ctx.args = args;

            return ctx.substances.getEffectSubstances(args);
        },
    },
    Substance: {
        async effects(data, args, ctx) {
            const substance = _.get(data, 'name');

            return ctx.substances.getSubstanceEffects(
                _.assign({}, { substance }, ctx.args),
            );
        },

        async uncertainInteractions(data, __, ctx) {
            const interactions = _.get(data, 'uncertainInteractions', null);

            if (!_.isArray(interactions)) {
                return null;
            }

            return Promise.all(interactions.map(
                async (substanceName) => {
                    const results = awaitctx.substances.getSubstances({
                        query: substanceName,
                        limit: 1,
                        offset: 0,
                    });

                    if (_.size(results) === 1) {
                        return results[0];
                    }

                    return {
                        name: substanceName,
                    };
                },
            ));
        },

        async unsafeInteractions(data, __, ctx) {
            const interactions = _.get(data, 'unsafeInteractions', null);

            if (!_.isArray(interactions)) {
                return null;
            }

            return Promise.all(interactions.map(
                async (substanceName) => {
                    const results = await ctx.substances.getSubstances({
                        query: substanceName,
                        limit: 1,
                        offset: 0,
                    });

                    if (_.size(results) === 1) {
                        return results[0];
                    }

                    return {
                        name: substanceName,
                    };
                },
            ));
        },

        async dangerousInteractions(data, __, ctx) {
            const interactions = _.get(data, 'dangerousInteractions', null);

            if (!Array.isArray(interactions)) {
                return null;
            }

            return Promise.all(interactions.map(
                async (substanceName) => {
                    const results = await ctx.substances.getSubstances({
                        query: substanceName,
                        limit: 1,
                        offset: 0,
                    });

                    if (_.size(results) === 1) {
                        return results[0];
                    }

                    return {
                        name: substanceName,
                    };
                },
            ));
        },

        async summary(data, args, ctx) {
            const substance = _.get(data, 'name');

            return ctx.substances.getSubstanceAbstract(
                _.assign({}, { substance }, ctx.args),
            );
        },

        async images(data, args, ctx) {
            const substance = _.get(data, 'name');

            return ctx.substances.getSubstanceImages(
                _.assign({}, { substance }, ctx.args),
            );
        },
    },
    Effect: {
        async substances(data, args, ctx) {
            const effect = _.get(data, 'name');

            return ctx.substances.getEffectSubstances(
                _.assign({}, { effect }, ctx.args),
            );
        },
    },
};

if (features.has('plebiscite')) {
    _.assign(baseResolvers.Query, {
        async erowid(data, { substance, offset, limit }, { plebiscite }) {
            return plebiscite.find({ substance, offset, limit });
        },
    });
}

class DeprecatedDirective extends SchemaDirectiveVisitor {
    visitArgumentDefinition(arg, { field }) {
        field.isDeprecated = true;
        field.deprecationReason = this.args.reason;
    }

    visitInputFieldDefinition(field) {
        field.isDeprecated = true;
        field.deprecationReason = this.args.reason;
    }

    visitFieldDefinition(field) {
        field.isDeprecated = true;
        field.deprecationReason = this.args.reason;
    }

    visitEnumValue(value) {
        value.isDeprecated = true;
        value.deprecationReason = this.args.reason;
    }
}

class PwEdge {
    get schema() {
        return makeExecutableSchema({
            typeDefs: [schema],
            resolvers: baseResolvers,
            schemaDirectives: {
                deprecated: DeprecatedDirective,
            },
        });
    }
}

module.exports = PwEdge;
