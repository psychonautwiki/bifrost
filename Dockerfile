FROM node:10.11.0-alpine

RUN mkdir -p /usr/src/app
WORKDIR /usr/src/app

ARG NODE_ENV
ENV NODE_ENV $NODE_ENV
COPY package.json /usr/src/app/
RUN yarn ; yarn cache clean
COPY . /usr/src/app

CMD [ "yarn", "start" ]
