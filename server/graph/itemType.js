'use strict';

const assert = require('assert');

const _ = require('lodash');
const Promise = require('bluebird');

const {makeExecutableSchema} = require('graphql-tools');

const schema = require('./schema/rootQuery');

const features = require('../util/features');

const _GeneratorFunction = (function*(){}).constructor;
const crMap = obj =>
    _.mapValues(obj, robj =>
        _.mapValues(robj, val =>
            val.constructor === _GeneratorFunction
                ? Promise.coroutine(val)
                : val
        )
    );

const baseResolvers = {
    Query: {
        * substances(data, args, ctx) {
            ctx.args = args;

            return yield* ctx.substances.getSubstances(args);
        },
        * effects(data, args, ctx) {
            ctx.args = args;

            return yield* ctx.substances.getEffects(args);
        }
    },
    Substance: {
        * effects(data, args, ctx) {
            const substance = _.get(data, 'name');

            return yield* ctx.substances.getSubstanceEffects(
                _.assign({}, {substance}, ctx.args)
            );
        }
    },
    Effect: {
        * substances(data, args, ctx) {
            const effect = _.get(data, 'name');

            return yield* ctx.substances.getEffectSubstances(
                _.assign({}, {effect}, ctx.args)
            );
        }
    }
};

if (features.has('plebiscite')) {
    _.assign(baseResolvers.Query, {
        * erowid(data, {substance, offset, limit}, {plebiscite}) {
            return yield* plebiscite.find({substance, offset, limit});
        }
    });
}

const resolvers = crMap(baseResolvers);

class PwEdge {
    get schema() {
        return makeExecutableSchema({
            typeDefs: [schema],
            resolvers
        });
    }
}

module.exports = PwEdge;
