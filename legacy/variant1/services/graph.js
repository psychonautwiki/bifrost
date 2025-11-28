'use strict';

const { ApolloServer } = require('apollo-server-express');
const { ApolloServerPluginLandingPageGraphQLPlayground } = require('apollo-server-core');

const featureContext = {};

const features = require('../util/features');

if (features.has('plebiscite')) {
    featureContext.plebiscite = require('../features/plebiscite');
}

const querySchema = require('../graph');

const Connector = require('../graph/storage/connector');
const { Substances } = require('../graph/storage/models');

const SMWDataArbitrator = require('../graph/helpers/smwDataArbitrator');
const smwDataArbitrator = new SMWDataArbitrator();

const PWPropParser = require('../graph/helpers/pwPropParser');

const pwPropParser = new PWPropParser({
    smwDataArbitrator,
});

module.exports = ({ app, log }) => {
    const baseQuerySchema = querySchema({ log });

    const server = new ApolloServer({
        schema: baseQuerySchema.schema,
        context:
            Object.assign(
                {},
                {
                    substances: new Substances({
                        connector: new Connector({ log }),
                        pwPropParser,
                        log,
                    }),
                },
                featureContext,
            ),

        formatError: (err) => {
            return {
                message: err.message,
                path: err.path,
                code: err.extensions.code,
            };
        },

        debug: true,

        plugins: [
            ApolloServerPluginLandingPageGraphQLPlayground({
                tabs: [
                    {
                        query: `{
    # Welcome to the PsychonautWiki API!
    #
    # To learn more about individual fields,
    # keep 'ctrl' (Windows) or 'cmd' (macOS)
    # pressed and click the field name. This
    # will open the respective documentation
    # entry in a sidebar on the right.
    #
    # If you have any questions or found an
    # issue or any bug, don't hesitate to
    # contact Kenan (kenan@psy.is).
    #
    # Happy hacking!

    substances(query: "Armodafinil") {
        name

        # routes of administration
        roas {
            name

            dose {
                units
                threshold
                heavy
                common { min max }
                light { min max }
                strong { min max }
            }

            duration {
                afterglow { min max units }
                comeup { min max units }
                duration { min max units }
                offset { min max units }
                onset { min max units }
                peak { min max units }
                total { min max units }
            }

            bioavailability {
                min max
            }
        }

        # subjective effects
        effects {
            name url
        }
    }
}`,
                        endpoint: '/',
                    },
                ],
            }),
        ],

        introspection: true,

        tracing: true,
        cacheControl: false,
    });

    return server
        .start()
        .then(() =>
            server.applyMiddleware({
                app,
                path: '/',
            }),
        );
};
