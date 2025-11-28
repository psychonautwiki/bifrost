'use strict';

const { gql } = require('apollo-server-express');

module.exports = ({ config }) => {
    const plebisciteEnabled = config.features.plebiscite.enabled;

    const plebisciteTypes = plebisciteEnabled ? `
        type ErowidMeta {
            erowidId: ID!
            gender: String
            published: String
            year: Int
            age: Int
            views: Int
        }

        type ErowidSubstanceInfo {
            amount: String
            method: String
            substance: String
            form: String
        }

        type Erowid {
            title: String
            author: String
            substance: String
            meta: ErowidMeta
            substanceInfo: [ErowidSubstanceInfo]
            erowidNotes: [String]
            pullQuotes: [String]
            body: String
        }
    ` : '';

    const plebisciteQuery = plebisciteEnabled ? `
        erowid(
            substance: String
            limit: Int=50
            offset: Int=0
        ): [Erowid]
    ` : '';

    return gql`
        type SubstanceClass {
            chemical: [String]
            psychoactive: [String]
        }

        type SubstanceTolerance {
            full: String
            half: String
            zero: String
        }

        interface RoaRange {
            min: Float
            max: Float
        }

        type SubstanceRoaRange implements RoaRange {
            min: Float
            max: Float
        }

        type SubstanceRoaDurationRange implements RoaRange {
            min: Float
            max: Float
            units: String
        }

        type SubstanceRoaDose {
            units: String
            threshold: Float
            heavy: Float
            common: SubstanceRoaRange
            light: SubstanceRoaRange
            strong: SubstanceRoaRange
        }

        type SubstanceRoaDuration {
            afterglow: SubstanceRoaDurationRange
            comeup: SubstanceRoaDurationRange
            duration: SubstanceRoaDurationRange
            offset: SubstanceRoaDurationRange
            onset: SubstanceRoaDurationRange
            peak: SubstanceRoaDurationRange
            total: SubstanceRoaDurationRange
        }

        type SubstanceRoa {
            name: String
            dose: SubstanceRoaDose
            duration: SubstanceRoaDuration
            bioavailability: SubstanceRoaRange
        }

        type SubstanceRoaTypes {
            oral: SubstanceRoa
            sublingual: SubstanceRoa
            buccal: SubstanceRoa
            insufflated: SubstanceRoa
            rectal: SubstanceRoa
            transdermal: SubstanceRoa
            subcutaneous: SubstanceRoa
            intramuscular: SubstanceRoa
            intravenous: SubstanceRoa
            smoked: SubstanceRoa
        }

        type SubstanceImage {
            thumb: String
            image: String
        }

        type Substance {
            name: String
            url: String
            featured: Boolean
            effects: [Effect]
            experiences: [Experience]
            class: SubstanceClass
            tolerance: SubstanceTolerance
            roa: SubstanceRoaTypes
            roas: [SubstanceRoa]
            summary: String
            images: [SubstanceImage]
            addictionPotential: String
            toxicity: [String]
            crossTolerances: [String]
            commonNames: [String]
            systematicName: String
            uncertainInteractions: [Substance]
            unsafeInteractions: [Substance]
            dangerousInteractions: [Substance]
        }

        type Effect {
            name: String
            url: String
            substances: [Substance]
            experiences: [Experience]
        }

        type Experience {
            substances: [Substance]
            effects: [Experience]
        }

        ${plebisciteTypes}

        type Query {
            substances(
                effect: String
                query: String
                chemicalClass: String
                psychoactiveClass: String
                limit: Int=10
                offset: Int=0
            ): [Substance]

            substances_by_effect(
                effect: [String]
                limit: Int=50
                offset: Int=0
            ): [Substance]

            effects_by_substance(
                substance: String
                limit: Int=50
                offset: Int=0
            ): [Effect]

            experiences(
                substances_by_effect: String,
                effects_by_substance: String,
                substance: String
            ): [Experience]

            ${plebisciteQuery}
        }
    `;
};
